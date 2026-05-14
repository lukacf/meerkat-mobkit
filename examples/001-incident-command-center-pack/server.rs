#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::collapsible_if,
    clippy::redundant_clone,
    clippy::needless_raw_string_hashes,
    clippy::single_match,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_pattern_matching,
    clippy::ignored_unit_patterns,
    clippy::clone_on_copy,
    clippy::manual_assert,
    clippy::unwrap_in_result,
    clippy::useless_vec
)]

#[path = "incident_command_center.rs"]
mod incident_command_center;

use incident_command_center::{
    build_runtime_bundle, incident_image_model, incident_model, scenario_path,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "meerkat_mobkit=info,axum=info".to_string()),
        )
        .with_target(false)
        .compact()
        .try_init()
        .ok();

    let _ = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "Set OPENAI_API_KEY to run the live incident command center example")?;
    println!(
        "incident command center model={} image_model={}",
        incident_model(),
        incident_image_model()
    );

    let bundle = build_runtime_bundle(&scenario_path()?).await?;
    let listen_addr = std::env::var("INCIDENT_COMMAND_CENTER_LISTEN_ADDR")
        .unwrap_or_else(|_| bundle.scenario.listen_addr.clone());
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    println!(
        "incident command center listening on http://{}",
        listen_addr
    );
    println!("GET  /console");
    println!("GET  /console/experience");
    println!("POST /console/rpc");
    println!("GET  /console/timeline/stream");
    bundle.runtime.serve(listener, bundle.decisions).await?;
    Ok(())
}
