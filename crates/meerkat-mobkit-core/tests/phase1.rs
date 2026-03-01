use std::process::Command;
use std::time::Duration;

use meerkat_mobkit_core::{
    normalize_event_line, route_module_call, route_module_call_rpc_json,
    route_module_call_rpc_subprocess, start_mobkit_runtime, start_mobkit_runtime_with_options,
    ConfigResolutionError, EventEnvelope, LifecycleStage, MobKitConfig, ModuleHealthState,
    ModuleRouteError, ModuleRouteRequest, ModuleRouteResponse, NormalizationError, RestartPolicy,
    RpcRouteError, RuntimeOptions, UnifiedEvent,
};
use serde_json::json;

fn shell_module(
    id: &str,
    script: &str,
    restart_policy: RestartPolicy,
) -> meerkat_mobkit_core::ModuleConfig {
    meerkat_mobkit_core::ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        restart_policy,
    }
}

#[test]
fn req_001_startup_ordering_and_graceful_shutdown_kills_tracked_children() {
    let config = MobKitConfig {
        modules: vec![shell_module(
            "mod-a",
            r#"printf '%s\n' "{\"event_id\":\"mod-a-evt\",\"source\":\"module\",\"timestamp_ms\":20,\"event\":{\"kind\":\"module\",\"module\":\"mod-a\",\"event_type\":\"ready\",\"payload\":{\"ok\":true,\"pid\":$$}}}"; exec sleep 30"#,
            RestartPolicy::Never,
        )],
        discovery: meerkat_mobkit_core::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec!["mod-a".to_string()],
        },
        pre_spawn: vec![],
    };

    let mut runtime =
        start_mobkit_runtime(config, vec![], Duration::from_secs(1)).expect("runtime starts");

    assert_eq!(
        runtime
            .lifecycle_events
            .iter()
            .map(|event| event.stage.clone())
            .collect::<Vec<_>>(),
        vec![
            LifecycleStage::MobStarted,
            LifecycleStage::ModulesStarted,
            LifecycleStage::MergedStreamStarted,
        ]
    );

    let pid = runtime
        .merged_events
        .iter()
        .find_map(|event| match &event.event {
            UnifiedEvent::Module(module) if module.module == "mod-a" => {
                module.payload.get("pid").and_then(|value| value.as_i64())
            }
            _ => None,
        })
        .expect("module pid should be present in payload");

    let shutdown = runtime.shutdown();
    assert_eq!(shutdown.orphan_processes, 0);
    assert_eq!(shutdown.terminated_modules, vec!["mod-a".to_string()]);
    assert!(!runtime.is_running());

    // OS-level proof that the module process is not alive after shutdown.
    let kill_status = Command::new("sh")
        .args(["-c", &format!("kill -0 {pid}")])
        .status()
        .expect("run kill -0");
    assert!(
        !kill_status.success(),
        "module process {pid} is still alive after shutdown"
    );

    assert_eq!(
        runtime
            .lifecycle_events
            .iter()
            .map(|event| event.stage.clone())
            .collect::<Vec<_>>(),
        vec![
            LifecycleStage::MobStarted,
            LifecycleStage::ModulesStarted,
            LifecycleStage::MergedStreamStarted,
            LifecycleStage::ShutdownRequested,
            LifecycleStage::ShutdownComplete,
        ]
    );
}

#[test]
fn req_002_supervisor_transitions_and_restart_policy_enforced_with_budgets() {
    let temp = tempfile::tempdir().expect("temp dir");
    let on_failure_state = temp.path().join("on-failure-state");

    let on_failure_script = format!(
        "if [ ! -f '{}' ]; then echo first > '{}'; exit 1; fi; if ! grep -q second '{}'; then echo second > '{}'; exit 1; fi; printf '%s\\n' '{{\"event_id\":\"on-failure-healthy\",\"source\":\"module\",\"timestamp_ms\":30,\"event\":{{\"kind\":\"module\",\"module\":\"on-failure\",\"event_type\":\"ready\",\"payload\":{{\"attempt\":3}}}}}}'",
        on_failure_state.display(),
        on_failure_state.display(),
        on_failure_state.display(),
        on_failure_state.display()
    );

    let always_script = "printf '%s\\n' '{\"event_id\":\"always-healthy\",\"source\":\"module\",\"timestamp_ms\":31,\"event\":{\"kind\":\"module\",\"module\":\"always\",\"event_type\":\"ready\",\"payload\":{\"attempt\":1}}}'".to_string();

    let config = MobKitConfig {
        modules: vec![
            shell_module("never", "exit 1", RestartPolicy::Never),
            shell_module("on-failure", &on_failure_script, RestartPolicy::OnFailure),
            shell_module("always", &always_script, RestartPolicy::Always),
        ],
        discovery: meerkat_mobkit_core::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec![
                "never".to_string(),
                "on-failure".to_string(),
                "always".to_string(),
            ],
        },
        pre_spawn: vec![],
    };

    let runtime = start_mobkit_runtime_with_options(
        config,
        vec![],
        Duration::from_secs(1),
        RuntimeOptions {
            on_failure_retry_budget: 2,
            always_restart_budget: 2,
            ..RuntimeOptions::default()
        },
    )
    .expect("runtime starts with supervisor transitions");

    let never = runtime
        .supervisor_report
        .transitions
        .iter()
        .filter(|transition| transition.module_id == "never")
        .map(|transition| transition.to.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        never,
        vec![
            ModuleHealthState::Starting,
            ModuleHealthState::Failed,
            ModuleHealthState::Stopped,
        ]
    );

    let on_failure = runtime
        .supervisor_report
        .transitions
        .iter()
        .filter(|transition| transition.module_id == "on-failure")
        .map(|transition| transition.to.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        on_failure,
        vec![
            ModuleHealthState::Starting,
            ModuleHealthState::Failed,
            ModuleHealthState::Restarting,
            ModuleHealthState::Failed,
            ModuleHealthState::Restarting,
            ModuleHealthState::Healthy,
        ]
    );

    let always = runtime
        .supervisor_report
        .transitions
        .iter()
        .filter(|transition| transition.module_id == "always")
        .map(|transition| transition.to.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        always,
        vec![
            ModuleHealthState::Starting,
            ModuleHealthState::Healthy,
            ModuleHealthState::Restarting,
            ModuleHealthState::Healthy,
            ModuleHealthState::Restarting,
            ModuleHealthState::Healthy,
        ]
    );
}

#[test]
fn req_003_event_bus_merges_agent_and_module_events_with_deterministic_order() {
    let config = MobKitConfig {
        modules: vec![
            shell_module(
                "mod-a",
                r#"printf '%s\n' '{"event_id":"evt-module-a","source":"module","timestamp_ms":20,"event":{"kind":"module","module":"mod-a","event_type":"ready","payload":{"m":"a"}}}'"#,
                RestartPolicy::Never,
            ),
            shell_module(
                "mod-b",
                r#"printf '%s\n' '{"event_id":"evt-module-b","source":"module","timestamp_ms":20,"event":{"kind":"module","module":"mod-b","event_type":"ready","payload":{"m":"b"}}}'"#,
                RestartPolicy::Never,
            ),
        ],
        discovery: meerkat_mobkit_core::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec!["mod-a".to_string(), "mod-b".to_string()],
        },
        pre_spawn: vec![],
    };

    let agent_events = vec![
        EventEnvelope {
            event_id: "evt-agent-early".to_string(),
            source: "agent".to_string(),
            timestamp_ms: 10,
            event: UnifiedEvent::Agent {
                agent_id: "a-1".to_string(),
                event_type: "heartbeat".to_string(),
            },
        },
        EventEnvelope {
            event_id: "evt-agent-mid".to_string(),
            source: "agent".to_string(),
            timestamp_ms: 20,
            event: UnifiedEvent::Agent {
                agent_id: "a-2".to_string(),
                event_type: "heartbeat".to_string(),
            },
        },
    ];

    let runtime =
        start_mobkit_runtime(config, agent_events, Duration::from_secs(1)).expect("runtime starts");

    assert_eq!(
        runtime
            .merged_events
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        vec![
            "evt-agent-early".to_string(),
            "evt-agent-mid".to_string(),
            "evt-module-a".to_string(),
            "evt-module-b".to_string(),
        ]
    );
    assert!(matches!(
        runtime.merged_events[0].event,
        UnifiedEvent::Agent { .. }
    ));
    assert!(matches!(
        runtime.merged_events[2].event,
        UnifiedEvent::Module(_)
    ));
}

#[test]
fn req_003_attribution_integrity_rejects_source_event_mismatch() {
    let mismatched = json!({
        "event_id": "evt-bad",
        "source": "agent",
        "timestamp_ms": 7,
        "event": {
            "kind": "module",
            "module": "mod-x",
            "event_type": "ready",
            "payload": {"ok": true}
        }
    })
    .to_string();

    let err = normalize_event_line(&mismatched).expect_err("mismatch should fail");
    assert_eq!(
        err,
        NormalizationError::SourceMismatch {
            expected: "module",
            got: "agent".to_string(),
        }
    );
}

#[test]
fn req_004_and_req_005_router_parity_library_and_rpc_with_typed_unloaded_error() {
    let config = MobKitConfig {
        modules: vec![shell_module(
            "router-mod",
            r#"printf '%s\n' '{"event_id":"evt-router","source":"module","timestamp_ms":55,"event":{"kind":"module","module":"router-mod","event_type":"response","payload":{"ok":true,"via":"module"}}}'"#,
            RestartPolicy::Never,
        )],
        discovery: meerkat_mobkit_core::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec!["router-mod".to_string()],
        },
        pre_spawn: vec![],
    };

    let runtime =
        start_mobkit_runtime(config, vec![], Duration::from_secs(1)).expect("runtime starts");

    let request = ModuleRouteRequest {
        module_id: "router-mod".to_string(),
        method: "echo".to_string(),
        params: json!({"msg":"hello"}),
    };

    let library_response = route_module_call(&runtime, &request, Duration::from_secs(1))
        .expect("library router call succeeds");
    assert_eq!(library_response.module_id, "router-mod");
    assert_eq!(library_response.method, "echo");
    assert_eq!(
        library_response.payload,
        json!({"ok": true, "via": "module"})
    );

    let rpc_response_json = route_module_call_rpc_json(
        &runtime,
        &serde_json::to_string(&request).expect("serialize request"),
        Duration::from_secs(1),
    )
    .expect("rpc wrapper succeeds");
    let rpc_response: ModuleRouteResponse =
        serde_json::from_str(&rpc_response_json).expect("deserialize rpc response");
    assert_eq!(rpc_response.module_id, "router-mod");
    assert_eq!(rpc_response.method, "echo");
    assert_eq!(rpc_response.payload, json!({"ok": true, "via": "module"}));

    let rpc_subprocess_response_json = route_module_call_rpc_subprocess(
        &runtime,
        "sh",
        &[
            "-c".to_string(),
            format!(
                "printf '%s\\n' '{}'",
                serde_json::to_string(&request).expect("serialize request")
            ),
        ],
        &[],
        Duration::from_secs(1),
    )
    .expect("rpc subprocess boundary succeeds");
    let rpc_subprocess_response: ModuleRouteResponse =
        serde_json::from_str(&rpc_subprocess_response_json)
            .expect("deserialize subprocess rpc response");
    assert_eq!(rpc_subprocess_response.module_id, "router-mod");
    assert_eq!(rpc_subprocess_response.method, "echo");
    assert_eq!(
        rpc_subprocess_response.payload,
        json!({"ok": true, "via": "module"})
    );

    let missing = ModuleRouteRequest {
        module_id: "missing".to_string(),
        method: "echo".to_string(),
        params: json!({}),
    };

    let library_error = route_module_call(&runtime, &missing, Duration::from_secs(1))
        .expect_err("missing module should fail");
    assert_eq!(
        library_error,
        ModuleRouteError::UnloadedModule("missing".to_string())
    );

    let rpc_error = route_module_call_rpc_json(
        &runtime,
        &serde_json::to_string(&missing).expect("serialize request"),
        Duration::from_secs(1),
    )
    .expect_err("rpc missing module should fail");
    assert_eq!(
        rpc_error,
        RpcRouteError::Route(ModuleRouteError::UnloadedModule("missing".to_string()))
    );

    let rpc_subprocess_error = route_module_call_rpc_subprocess(
        &runtime,
        "sh",
        &[
            "-c".to_string(),
            format!(
                "printf '%s\\n' '{}'",
                serde_json::to_string(&missing).expect("serialize request")
            ),
        ],
        &[],
        Duration::from_secs(1),
    )
    .expect_err("rpc subprocess missing module should fail");
    assert_eq!(
        rpc_subprocess_error,
        RpcRouteError::Route(ModuleRouteError::UnloadedModule("missing".to_string()))
    );
}

#[test]
fn req_001_config_error_when_discovery_references_unknown_module() {
    let config = MobKitConfig {
        modules: vec![],
        discovery: meerkat_mobkit_core::DiscoverySpec {
            namespace: "default".to_string(),
            modules: vec!["ghost".to_string()],
        },
        pre_spawn: vec![],
    };

    let error = start_mobkit_runtime(config, vec![], Duration::from_secs(1))
        .expect_err("unknown module should fail startup");
    assert_eq!(
        error,
        meerkat_mobkit_core::MobkitRuntimeError::Config(
            ConfigResolutionError::ModuleNotConfigured("ghost".to_string())
        )
    );
}
