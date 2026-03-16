//! Module process boundary — MCP connections, environment setup, and tool probing.

use std::collections::HashMap;
use std::time::Duration;

use meerkat_core::ContentBlock;
use meerkat_mcp::{McpConnection, McpError, McpServerConfig};
use serde_json::Value;
use tokio::time::timeout;

use super::*;

pub(super) const MODULE_BOUNDARY_KIND_ENV: &str = "MOBKIT_MODULE_BOUNDARY";
pub(super) const MODULE_BOUNDARY_KIND_MCP: &str = "mcp";
pub(super) const CORE_MODULE_MCP_TIMEOUT: Duration = Duration::from_secs(1);
pub(super) const ROUTER_RESOLVE_MCP_TOOL: &str = "routing.resolve";
pub(super) const DELIVERY_SEND_MCP_TOOL: &str = "delivery.send";
pub(super) const MEMORY_CONFLICT_READ_MCP_TOOL: &str = "memory.conflict_read";
pub(super) const SCHEDULING_DISPATCH_MCP_TOOL: &str = "scheduling.dispatch";

pub(super) fn module_uses_mcp(module: &ModuleConfig, pre_spawn: Option<&PreSpawnData>) -> bool {
    pre_spawn
        .filter(|data| data.module_id == module.id)
        .and_then(|data| {
            data.env
                .iter()
                .find(|(key, _)| key == MODULE_BOUNDARY_KIND_ENV)
                .map(|(_, value)| value)
        })
        .map(|value| value.trim().eq_ignore_ascii_case(MODULE_BOUNDARY_KIND_MCP))
        .unwrap_or(false)
}

pub(super) fn module_env_with_extra(
    module: &ModuleConfig,
    pre_spawn: Option<&PreSpawnData>,
    extra_env: &[(String, String)],
) -> Vec<(String, String)> {
    pre_spawn
        .filter(|data| data.module_id == module.id)
        .map(|data| data.env.clone())
        .unwrap_or_default()
        .into_iter()
        .chain(extra_env.iter().cloned())
        .collect::<Vec<_>>()
}

pub(super) fn mcp_required_error(module_id: &str, flow: &str) -> RuntimeBoundaryError {
    RuntimeBoundaryError::Mcp(McpBoundaryError::McpRequired {
        module_id: module_id.to_string(),
        flow: flow.to_string(),
    })
}

pub(super) fn probe_module_mcp_tools(
    module: &ModuleConfig,
    pre_spawn: Option<&PreSpawnData>,
    timeout_duration: Duration,
) -> Result<Vec<String>, RuntimeBoundaryError> {
    let env = module_env_with_extra(module, pre_spawn, &[]);
    let env_map = env.into_iter().collect::<HashMap<_, _>>();
    let config = McpServerConfig::stdio(
        format!("mobkit-{}", module.id),
        module.command.clone(),
        module.args.clone(),
        env_map,
    );
    let module_id = module.id.clone();

    run_in_tokio_runtime(async move {
        let connection = connect_with_timeout(&module_id, &config, timeout_duration).await?;
        let tools_result = list_tools_with_timeout(&module_id, &connection, timeout_duration).await;
        let close_result = close_with_timeout(&module_id, connection, timeout_duration).await;
        let tools = finalize_mcp_operation_with_close(tools_result, close_result)?;
        let mut tool_names = tools.into_iter().map(|tool| tool.name).collect::<Vec<_>>();
        tool_names.sort();
        Ok(tool_names)
    })
}

pub(super) fn call_module_mcp_tool_text(
    module: &ModuleConfig,
    pre_spawn: Option<&PreSpawnData>,
    tool_name: &str,
    args: &Value,
    timeout_duration: Duration,
) -> Result<String, RuntimeBoundaryError> {
    let env = module_env_with_extra(module, pre_spawn, &[]);
    let env_map = env.into_iter().collect::<HashMap<_, _>>();
    let config = McpServerConfig::stdio(
        format!("mobkit-{}", module.id),
        module.command.clone(),
        module.args.clone(),
        env_map,
    );
    let module_id = module.id.clone();
    let requested_tool = tool_name.to_string();
    let args = args.clone();

    run_in_tokio_runtime(async move {
        let connection = connect_with_timeout(&module_id, &config, timeout_duration).await?;
        let operation_result = async {
            let tools = list_tools_with_timeout(&module_id, &connection, timeout_duration).await?;
            let available_tools = tools
                .iter()
                .map(|tool| tool.name.clone())
                .collect::<Vec<_>>();
            if !available_tools.iter().any(|name| name == &requested_tool) {
                return Err(RuntimeBoundaryError::Mcp(McpBoundaryError::ToolNotFound {
                    module_id: module_id.clone(),
                    tool: requested_tool.clone(),
                    available_tools,
                }));
            }
            call_tool_with_timeout(
                &module_id,
                &requested_tool,
                &connection,
                &args,
                timeout_duration,
            )
            .await
        }
        .await;

        let close_result = close_with_timeout(&module_id, connection, timeout_duration).await;
        finalize_mcp_operation_with_close(operation_result, close_result)
    })
}

pub(super) fn call_module_mcp_tool_json(
    module: &ModuleConfig,
    pre_spawn: Option<&PreSpawnData>,
    tool_name: &str,
    args: &Value,
    timeout_duration: Duration,
) -> Result<Value, RuntimeBoundaryError> {
    let response = call_module_mcp_tool_text(module, pre_spawn, tool_name, args, timeout_duration)?;
    serde_json::from_str::<Value>(&response).map_err(|_| {
        RuntimeBoundaryError::Mcp(McpBoundaryError::InvalidJsonResponse {
            module_id: module.id.clone(),
            tool: tool_name.to_string(),
            response,
        })
    })
}

fn run_in_tokio_runtime<F, T>(future: F) -> Result<T, RuntimeBoundaryError>
where
    F: std::future::Future<Output = Result<T, RuntimeBoundaryError>>,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(RuntimeBoundaryError::Mcp(
            McpBoundaryError::RuntimeUnavailable(
                "cannot execute blocking MCP boundary call inside an active tokio runtime"
                    .to_string(),
            ),
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            RuntimeBoundaryError::Mcp(McpBoundaryError::RuntimeUnavailable(error.to_string()))
        })?;
    runtime.block_on(future)
}

async fn connect_with_timeout(
    module_id: &str,
    config: &McpServerConfig,
    timeout_duration: Duration,
) -> Result<McpConnection, RuntimeBoundaryError> {
    timeout(timeout_duration, McpConnection::connect(config))
        .await
        .map_err(|_| mcp_timeout_error(module_id, "connect", timeout_duration))?
        .map_err(|error| mcp_connect_error(module_id, error))
}

async fn list_tools_with_timeout(
    module_id: &str,
    connection: &McpConnection,
    timeout_duration: Duration,
) -> Result<Vec<meerkat_core::ToolDef>, RuntimeBoundaryError> {
    timeout(timeout_duration, connection.list_tools())
        .await
        .map_err(|_| mcp_timeout_error(module_id, "list_tools", timeout_duration))?
        .map_err(|error| mcp_list_tools_error(module_id, error))
}

/// Flatten `Vec<ContentBlock>` to a single text string for MobKit's JSON pipeline.
///
/// MCP module tools return text in practice. If the result contains only
/// text blocks, concatenate them. If non-text blocks are present, serialize
/// the full block list to JSON and log a warning — this is lossy and will
/// be replaced when the pipeline supports multimodal content natively.
fn content_blocks_to_text(blocks: Vec<ContentBlock>) -> String {
    let all_text = blocks
        .iter()
        .all(|b| matches!(b, ContentBlock::Text { .. }));
    if all_text {
        blocks
            .into_iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text),
                _ => None,
            })
            .collect::<String>()
    } else {
        eprintln!(
            "WARN mobkit: MCP tool returned multimodal content blocks; \
             serializing to JSON string (lossy) until pipeline supports native multimodal"
        );
        serde_json::to_string(&blocks).unwrap_or_default()
    }
}

async fn call_tool_with_timeout(
    module_id: &str,
    tool_name: &str,
    connection: &McpConnection,
    args: &Value,
    timeout_duration: Duration,
) -> Result<String, RuntimeBoundaryError> {
    let blocks = timeout(timeout_duration, connection.call_tool(tool_name, args))
        .await
        .map_err(|_| {
            mcp_timeout_error(
                module_id,
                &format!("call_tool:{tool_name}"),
                timeout_duration,
            )
        })?
        .map_err(|error| mcp_tool_call_error(module_id, tool_name, error))?;
    Ok(content_blocks_to_text(blocks))
}

async fn close_with_timeout(
    module_id: &str,
    connection: McpConnection,
    timeout_duration: Duration,
) -> Result<(), RuntimeBoundaryError> {
    timeout(timeout_duration, connection.close())
        .await
        .map_err(|_| mcp_timeout_error(module_id, "close", timeout_duration))?
        .map_err(|error| {
            RuntimeBoundaryError::Mcp(McpBoundaryError::CloseFailed {
                module_id: module_id.to_string(),
                reason: error.to_string(),
            })
        })
}

fn mcp_timeout_error(
    module_id: &str,
    operation: &str,
    timeout_duration: Duration,
) -> RuntimeBoundaryError {
    RuntimeBoundaryError::Mcp(McpBoundaryError::Timeout {
        module_id: module_id.to_string(),
        operation: operation.to_string(),
        timeout_ms: timeout_duration.as_millis() as u64,
    })
}

fn mcp_connect_error(module_id: &str, error: McpError) -> RuntimeBoundaryError {
    RuntimeBoundaryError::Mcp(McpBoundaryError::ConnectionFailed {
        module_id: module_id.to_string(),
        reason: error.to_string(),
    })
}

fn mcp_list_tools_error(module_id: &str, error: McpError) -> RuntimeBoundaryError {
    RuntimeBoundaryError::Mcp(McpBoundaryError::ToolListFailed {
        module_id: module_id.to_string(),
        reason: error.to_string(),
    })
}

fn mcp_tool_call_error(module_id: &str, tool_name: &str, error: McpError) -> RuntimeBoundaryError {
    RuntimeBoundaryError::Mcp(McpBoundaryError::ToolCallFailed {
        module_id: module_id.to_string(),
        tool: tool_name.to_string(),
        reason: error.to_string(),
    })
}

fn finalize_mcp_operation_with_close<T>(
    operation_result: Result<T, RuntimeBoundaryError>,
    close_result: Result<(), RuntimeBoundaryError>,
) -> Result<T, RuntimeBoundaryError> {
    match (operation_result, close_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(close_error)) => Err(close_error),
        (Err(operation_error), Ok(())) => Err(operation_error),
        (Err(operation_error), Err(close_error)) => {
            Err(attach_mcp_close_failure(operation_error, close_error))
        }
    }
}

fn attach_mcp_close_failure(
    operation_error: RuntimeBoundaryError,
    close_error: RuntimeBoundaryError,
) -> RuntimeBoundaryError {
    match (operation_error, close_error) {
        (RuntimeBoundaryError::Mcp(primary), RuntimeBoundaryError::Mcp(close)) => {
            RuntimeBoundaryError::Mcp(McpBoundaryError::OperationFailedWithCloseFailure {
                primary: Box::new(primary),
                close: Box::new(close),
            })
        }
        (operation_error, _) => operation_error,
    }
}
