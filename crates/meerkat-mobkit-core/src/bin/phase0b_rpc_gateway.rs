use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use meerkat_mobkit_core::{
    handle_mobkit_rpc_json, handle_unified_rpc_json, start_mobkit_runtime, AuthPolicy,
    BigQueryNaming, ConsolePolicy, DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec,
    MobKitConfig, ModuleConfig, ReleaseMetadata, RestartPolicy, RuntimeDecisionState,
    RuntimeOpsPolicy, TrustedOidcRuntimeConfig, UnifiedRuntime,
};

use async_trait::async_trait;
use meerkat::{
    AgentEvent, AgentFactory, Config, CreateSessionRequest, EphemeralSessionService,
    FactoryAgent, FactoryAgentBuilder, SessionAgentBuilder, SessionError,
};
use meerkat_core::error::{AgentError, ToolError};
use meerkat_core::types::{ToolCallView, ToolDef, ToolResult};
use meerkat_core::AgentToolDispatcher;
use meerkat_mob::{MobDefinition, MobStorage};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex};

fn minimal_decision_state() -> RuntimeDecisionState {
    RuntimeDecisionState {
        bigquery: BigQueryNaming {
            dataset: "default_dataset".to_string(),
            table: "default_table".to_string(),
        },
        modules: vec![],
        auth: AuthPolicy::default(),
        trusted_oidc: TrustedOidcRuntimeConfig {
            discovery_json: r#"{"issuer":"https://noop.example.com","authorization_endpoint":"https://noop.example.com/auth","token_endpoint":"https://noop.example.com/token","jwks_uri":"https://noop.example.com/.well-known/jwks.json","response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"]}"#.to_string(),
            jwks_json: r#"{"keys":[]}"#.to_string(),
            audience: "persistent-gateway".to_string(),
        },
        console: ConsolePolicy::default(),
        ops: RuntimeOpsPolicy::default(),
        release_metadata: ReleaseMetadata {
            targets: vec![
                "crates.io".to_string(),
                "npm".to_string(),
                "pypi".to_string(),
                "github-releases".to_string(),
            ],
            support_matrix: "lts".to_string(),
        },
    }
}

fn shell_module(id: &str, script: &str) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        restart_policy: RestartPolicy::Never,
    }
}

/// Original single-shot mode: reads request from env, runs once, prints response.
fn run_single_shot() {
    let request = std::env::var("MOBKIT_RPC_REQUEST")
        .expect("MOBKIT_RPC_REQUEST must be set for phase0b_rpc_gateway");

    let config = MobKitConfig {
        modules: vec![shell_module(
            "routing",
            r#"printf '%s\n' '{"event_id":"evt-routing","source":"module","timestamp_ms":101,"event":{"kind":"module","module":"routing","event_type":"ready","payload":{"family":"routing","health":{"state":"healthy"},"tools":{"list_method":"routing/tools.list","representative_call":{"method":"routing/tool.call","params_schema":{"tool":"string","input":"json"}}}}}}'"#,
        )],
        discovery: DiscoverySpec {
            namespace: "phase0b-rpc".to_string(),
            modules: vec!["routing".to_string()],
        },
        pre_spawn: vec![],
    };

    let mut runtime =
        start_mobkit_runtime(config, vec![], Duration::from_secs(1)).expect("runtime starts");
    let response = handle_mobkit_rpc_json(&mut runtime, &request, Duration::from_secs(1));
    print!("{response}");
    let _ = runtime.shutdown();
}

// ---------------------------------------------------------------------------
// StdioCallbackAgentBuilder — wraps FactoryAgentBuilder, sends callback/build_agent
// to Python over stdout before building the agent.
// ---------------------------------------------------------------------------

/// Shared handle for sending lines to stdout and receiving callback responses.
#[derive(Clone)]
struct StdioCallbackBridge {
    /// Send a line to stdout (the stdout writer task reads from this).
    stdout_tx: mpsc::Sender<String>,
    /// Pending callback responses keyed by callback ID.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    /// Counter for generating unique callback IDs.
    counter: Arc<std::sync::atomic::AtomicU64>,
}

impl StdioCallbackBridge {
    fn new(stdout_tx: mpsc::Sender<String>) -> Self {
        Self {
            stdout_tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
            counter: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Send a callback request to Python and wait for the response.
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id_str = format!("cb-{id}");

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id_str.clone(), tx);

        let request = json!({
            "jsonrpc": "2.0",
            "id": id_str,
            "method": method,
            "params": params,
        });
        let line = match serde_json::to_string(&request) {
            Ok(l) => l,
            Err(e) => {
                self.pending.lock().await.remove(&id_str);
                return Err(e.to_string());
            }
        };
        if let Err(_) = self.stdout_tx.send(line).await {
            self.pending.lock().await.remove(&id_str);
            return Err("stdout channel closed".to_string());
        }

        // Wait for Python to respond (routed by the stdin multiplexer)
        match tokio::time::timeout(Duration::from_secs(120), rx).await {
            Ok(Ok(value)) => {
                if let Some(error) = value.get("error") {
                    Err(format!(
                        "callback error: {}",
                        error.get("message").and_then(|m| m.as_str()).unwrap_or("unknown")
                    ))
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                }
            }
            Ok(Err(_)) => Err("callback response channel dropped".to_string()),
            Err(_) => {
                self.pending.lock().await.remove(&id_str);
                Err("callback timed out after 120s".to_string())
            }
        }
    }

    /// Route an incoming callback response (has "id" starting with "cb-").
    async fn route_callback_response(&self, msg: Value) {
        let id = msg
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(tx) = self.pending.lock().await.remove(&id) {
            let _ = tx.send(msg);
        }
    }
}

/// Tool dispatcher that routes tool calls to Python via the callback bridge.
///
/// Created from tool name strings provided by `SessionBuildOptions.add_tools()`.
/// When the agent calls a tool, `dispatch()` sends `callback/call_tool` to Python
/// and returns the result.
struct CallbackToolDispatcher {
    bridge: StdioCallbackBridge,
    tool_defs: Arc<[Arc<ToolDef>]>,
}

impl CallbackToolDispatcher {
    fn new(bridge: StdioCallbackBridge, tool_names: Vec<String>) -> Self {
        let tool_defs: Vec<Arc<ToolDef>> = tool_names
            .into_iter()
            .map(|name| {
                Arc::new(ToolDef {
                    name,
                    description: "Python callback tool".to_string(),
                    input_schema: json!({"type": "object"}),
                })
            })
            .collect();
        Self {
            bridge,
            tool_defs: tool_defs.into(),
        }
    }
}

#[async_trait]
impl AgentToolDispatcher for CallbackToolDispatcher {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        Arc::clone(&self.tool_defs)
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolResult, ToolError> {
        let args: Value = serde_json::from_str(call.args.get()).map_err(|e| {
            ToolError::InvalidArguments {
                name: call.name.to_string(),
                reason: e.to_string(),
            }
        })?;
        let params = json!({
            "tool": call.name,
            "arguments": args,
        });
        match self.bridge.call("callback/call_tool", params).await {
            Ok(result) => {
                let content = result
                    .get("content")
                    .map(|v| {
                        if let Some(s) = v.as_str() {
                            s.to_string()
                        } else {
                            serde_json::to_string(v).unwrap_or_default()
                        }
                    })
                    .unwrap_or_else(|| serde_json::to_string(&result).unwrap_or_default());
                Ok(ToolResult {
                    tool_use_id: call.id.to_string(),
                    content,
                    is_error: false,
                })
            }
            Err(err) => Ok(ToolResult {
                tool_use_id: call.id.to_string(),
                content: format!("Tool execution failed: {err}"),
                is_error: true,
            }),
        }
    }
}

/// Wraps FactoryAgentBuilder — sends callback/build_agent to Python before building.
struct StdioCallbackAgentBuilder {
    inner: FactoryAgentBuilder,
    bridge: StdioCallbackBridge,
    has_session_builder: bool,
}

#[async_trait]
impl SessionAgentBuilder for StdioCallbackAgentBuilder {
    type Agent = FactoryAgent;

    async fn build_agent(
        &self,
        req: &CreateSessionRequest,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Self::Agent, SessionError> {
        if !self.has_session_builder {
            return self.inner.build_agent(req, event_tx).await;
        }

        // Send callback to Python with full session context.
        // app_context flows from SpawnMemberSpec.context → build.app_context.
        let options = json!({
            "session_id": req.labels.as_ref().and_then(|l| l.get("session_id")),
            "model": &req.model,
            "prompt": &req.prompt,
            "labels": &req.labels,
            "app_context": req.build.as_ref()
                .and_then(|b| b.app_context.as_ref()),
        });
        let params = json!({ "options": options });
        let callback_result = self.bridge.call("callback/build_agent", params).await;

        match callback_result {
            Ok(result) => {
                // Apply Python-returned options to a cloned request
                let mut modified_req = CreateSessionRequest {
                    model: req.model.clone(),
                    prompt: req.prompt.clone(),
                    system_prompt: req.system_prompt.clone(),
                    max_tokens: req.max_tokens,
                    event_tx: req.event_tx.clone(),
                    host_mode: req.host_mode,
                    skill_references: req.skill_references.clone(),
                    initial_turn: req.initial_turn.clone(),
                    build: req.build.clone(),
                    labels: req.labels.clone(),
                };
                // Apply additional_instructions as system prompt extension
                if let Some(instructions) = result.get("additional_instructions") {
                    if let Some(arr) = instructions.as_array() {
                        let combined: Vec<&str> = arr
                            .iter()
                            .filter_map(|v| v.as_str())
                            .collect();
                        if !combined.is_empty() {
                            let extra = combined.join("\n");
                            modified_req.system_prompt = Some(match &modified_req.system_prompt {
                                Some(existing) => format!("{existing}\n{extra}"),
                                None => extra,
                            });
                        }
                    }
                }
                // Apply labels
                if let Some(labels) = result.get("labels").and_then(|v| v.as_object()) {
                    let label_map = modified_req.labels.get_or_insert_with(Default::default);
                    for (k, v) in labels {
                        if let Some(s) = v.as_str() {
                            label_map.insert(k.clone(), s.to_string());
                        }
                    }
                }
                // Callback tools: Python SDK provides tool names via add_tools()
                // or register_tool(). Create a CallbackToolDispatcher that routes
                // tool calls back to Python via callback/call_tool.
                if let Some(tools) = result.get("tools") {
                    match tools.as_array() {
                        Some(arr) => {
                            let mut tool_names = Vec::with_capacity(arr.len());
                            for v in arr {
                                if let Some(name) = v.as_str() {
                                    tool_names.push(name.to_string());
                                } else {
                                    return Err(SessionError::Agent(AgentError::ToolError(format!(
                                        "callback/build_agent: tools must be strings, got: {v}"
                                    ))));
                                }
                            }
                            if !tool_names.is_empty() {
                                let dispatcher = CallbackToolDispatcher::new(
                                    self.bridge.clone(),
                                    tool_names,
                                );
                                let build = modified_req.build.get_or_insert_with(|| {
                                    meerkat_core::service::SessionBuildOptions::default()
                                });
                                build.external_tools = Some(Arc::new(dispatcher));
                            }
                        }
                        None => {
                            return Err(SessionError::Agent(AgentError::ToolError(format!(
                                "callback/build_agent: tools must be a JSON array, got: {tools}"
                            ))));
                        }
                    }
                }
                self.inner.build_agent(&modified_req, event_tx).await
            }
            Err(err) => {
                eprintln!("callback/build_agent failed: {err}");
                // Continue with default build — don't fail the session
                self.inner.build_agent(req, event_tx).await
            }
        }
    }
}

/// Persistent mode: reads JSON-RPC over stdin, bootstraps unified runtime, serves HTTP.
#[tokio::main]
async fn run_persistent() {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);

    // 1. Read first line — must be mobkit/init
    let mut init_line = String::new();
    if reader.read_line(&mut init_line).await.unwrap_or(0) == 0 {
        eprintln!("phase0b_rpc_gateway: stdin closed before init request");
        std::process::exit(1);
    }

    let init_raw: Value = match serde_json::from_str(init_line.trim()) {
        Ok(v) => v,
        Err(e) => {
            let error_response = json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": format!("Parse error: {e}") }
            });
            println!("{}", serde_json::to_string(&error_response).unwrap());
            std::process::exit(1);
        }
    };

    let request_id = init_raw.get("id").cloned().unwrap_or(Value::Null);
    let method = init_raw
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    if method != "mobkit/init" {
        let error_response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": { "code": -32600, "message": format!("Expected mobkit/init, got {method}") }
        });
        println!("{}", serde_json::to_string(&error_response).unwrap());
        std::process::exit(1);
    }

    let params = init_raw
        .get("params")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // 2. Parse init params
    let mob_config_toml = params
        .get("mob_config")
        .and_then(|v| v.as_str())
        .unwrap_or(
            r#"
[mob]
id = "persistent-gateway"

[profiles.default]
model = "gpt-5.2"
external_addressable = true
"#,
        );

    let definition = MobDefinition::from_toml(mob_config_toml).unwrap_or_else(|e| {
        let error_response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": { "code": -32602, "message": format!("Invalid mob_config TOML: {e}") }
        });
        println!("{}", serde_json::to_string(&error_response).unwrap());
        std::process::exit(1);
    });

    let modules: Vec<ModuleConfig> = params
        .get("modules")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let discovery_modules: Vec<String> = modules.iter().map(|m| m.id.clone()).collect();
    let module_config = MobKitConfig {
        modules,
        discovery: DiscoverySpec {
            namespace: "persistent-gateway".to_string(),
            modules: discovery_modules,
        },
        pre_spawn: vec![],
    };

    let has_session_builder = params
        .get("has_session_builder")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 3. Set up stdout writer channel for multiplexed output
    let (stdout_tx, mut stdout_rx) = mpsc::channel::<String>(64);
    let stdout_writer = tokio::spawn(async move {
        while let Some(line) = stdout_rx.recv().await {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{line}");
            let _ = stdout.flush();
            drop(stdout); // release lock before next await
        }
    });

    // 4. Build callback bridge and start stdin multiplexer BEFORE bootstrap.
    // This ensures callback responses (e.g. callback/build_agent during discovery
    // spawn) are routed even while UnifiedRuntime::bootstrap is running.
    let bridge = StdioCallbackBridge::new(stdout_tx.clone());
    let (rpc_tx, mut rpc_rx) = mpsc::channel::<String>(64);

    let stdin_reader = tokio::spawn({
        let bridge = bridge.clone();
        let rpc_tx = rpc_tx.clone();
        async move {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {}
                    Err(_) => break,
                }
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let msg: Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                // Callback responses: "id" starts with "cb-" and no "method"
                let is_callback_response = msg
                    .get("id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id.starts_with("cb-"))
                    && msg.get("method").is_none();

                if is_callback_response {
                    bridge.route_callback_response(msg).await;
                } else {
                    // Queue RPC request for the dispatch loop
                    let _ = rpc_tx.send(trimmed.to_string()).await;
                }
            }
        }
    });

    // 5. Build session service with callback bridge
    // AgentFactory needs a working directory for agent scratch space even with
    // EphemeralSessionService (sessions don't persist, but agents use the path
    // during execution). temp_dir must outlive runtime — dropped after shutdown.
    let temp_dir = tempfile::tempdir().expect("create temp dir for agent working space");
    let factory = AgentFactory::new(temp_dir.path()).comms(true);
    let inner_builder = FactoryAgentBuilder::new(factory, Config::default());
    let callback_builder = StdioCallbackAgentBuilder {
        inner: inner_builder,
        bridge: bridge.clone(),
        has_session_builder,
    };
    let session_service = Arc::new(EphemeralSessionService::new(callback_builder, 16));

    let mob_spec =
        MobBootstrapSpec::new(definition, MobStorage::in_memory(), session_service).with_options(
            MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: None,
            },
        );

    let timeout = Duration::from_secs(30);
    let mut runtime = UnifiedRuntime::bootstrap(mob_spec, module_config, timeout)
        .await
        .unwrap_or_else(|e| {
            let error_response = json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "error": { "code": -32603, "message": format!("Runtime bootstrap failed: {e}") }
            });
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{}", serde_json::to_string(&error_response).unwrap());
            let _ = stdout.flush();
            std::process::exit(1);
        });

    // 6. Bind HTTP server on ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    let http_base_url = format!("http://127.0.0.1:{port}");

    // 7. Start HTTP with graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let app = runtime.build_reference_app_router(minimal_decision_state());
    let serve_task = tokio::spawn({
        let mut shutdown_rx = shutdown_rx.clone();
        async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_rx.changed().await.ok();
                })
                .await
        }
    });

    // 8. Send init response via stdout channel
    let loaded_modules = runtime.loaded_modules();
    let init_response = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "http_base_url": http_base_url,
            "loaded_modules": loaded_modules,
        }
    });
    let _ = stdout_tx
        .send(serde_json::to_string(&init_response).unwrap())
        .await;

    // 9. RPC dispatch loop: process queued requests from the stdin reader task
    {
        loop {
            let request_line = tokio::select! {
                line = rpc_rx.recv() => match line {
                    Some(l) => l,
                    None => break, // stdin reader closed (EOF or error)
                },
                _ = tokio::signal::ctrl_c() => break,
            };
            let response = handle_unified_rpc_json(
                &mut runtime,
                &request_line,
                timeout,
                Some(&http_base_url),
            )
            .await;
            if !response.is_empty() {
                let _ = stdout_tx.send(response).await;
            }
        }
    }

    // 10. Graceful shutdown: stop HTTP server, then runtime
    let _ = shutdown_tx.send(true);
    let _ = serve_task.await;
    runtime.shutdown().await;
    drop(rpc_tx);
    drop(stdout_tx);
    let _ = stdin_reader.await;
    let _ = stdout_writer.await;
    drop(temp_dir);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--persistent") {
        run_persistent();
    } else {
        run_single_shot();
    }
}
