use std::net::SocketAddr;

use meerkat_mobkit::flow_editor_router;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listen = listen_addr()?;
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let addr = listener.local_addr()?;
    eprintln!("mobkit flow editor listening on http://{addr}/flow-editor");
    axum::serve(listener, flow_editor_router()).await?;
    Ok(())
}

fn listen_addr() -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => {
                let value = args.next().ok_or("--listen requires HOST:PORT")?;
                return Ok(value.parse()?);
            }
            "--help" | "-h" => {
                eprintln!("Usage: mobkit_flow_editor [--listen HOST:PORT]");
                std::process::exit(0);
            }
            other => {
                return Err(format!("unknown argument: {other}").into());
            }
        }
    }
    Ok(std::env::var("MOBKIT_FLOW_EDITOR_LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:4191".to_string())
        .parse()?)
}
