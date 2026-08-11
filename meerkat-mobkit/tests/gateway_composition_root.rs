#![allow(clippy::panic)]

//! Structural guard for simplification item 10. The compatibility binaries
//! keep distinct wire/config adapters, but neither may reacquire runtime
//! bootstrap, HTTP admission, or shutdown ownership.

const CONSOLE_GATEWAY: &str = include_str!("../src/bin/mobkit_gateway.rs");
const STDIO_GATEWAY: &str = include_str!("../src/bin/rpc_gateway.rs");

#[test]
fn both_binaries_delegate_runtime_and_http_lifecycle_to_the_library_root() {
    for (name, source) in [
        ("mobkit_gateway", CONSOLE_GATEWAY),
        ("rpc_gateway", STDIO_GATEWAY),
    ] {
        assert!(
            source.contains("GatewayComposition::prepare"),
            "{name} bypassed the shared typed composition root"
        );
        assert!(
            source.contains("GatewayHttpBinding::bind_loopback"),
            "{name} bypassed shared HTTP admission"
        );
        assert!(
            source.contains("let shutdown = composition"),
            "{name} bypassed ordered cooperative shutdown"
        );
        assert!(
            !source.contains("Box::pin(UnifiedRuntime::bootstrap("),
            "{name} reacquired a direct bootstrap root"
        );
        assert!(
            !source.contains("Box::pin(UnifiedRuntime::bootstrap_with_options("),
            "{name} reacquired a direct bootstrap-with-options root"
        );
        assert!(
            !source.contains("axum::serve("),
            "{name} reacquired direct HTTP lifecycle ownership"
        );
    }
}

#[test]
fn compatibility_only_command_surfaces_remain_on_their_original_binary() {
    assert!(CONSOLE_GATEWAY.contains("storage-migrate"));
    assert!(CONSOLE_GATEWAY.contains("storage-prune"));
    assert!(!CONSOLE_GATEWAY.contains("MOBKIT_RPC_REQUEST"));

    assert!(STDIO_GATEWAY.contains("MOBKIT_RPC_REQUEST"));
    assert!(STDIO_GATEWAY.contains("run_single_shot"));
    assert!(!STDIO_GATEWAY.contains("storage-migrate"));
    assert!(!STDIO_GATEWAY.contains("storage-prune"));
}
