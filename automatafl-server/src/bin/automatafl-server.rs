//! Standalone binary for the Automatafl Fog-of-War Platform Server (`R5` / `R6`).

use std::env;
use std::net::SocketAddr;

use automatafl_server::api::{build_router, AppState};
use automatafl_server::persistence::GameStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bind_addr = env::var("AUTOMATAFL_BIND").unwrap_or_else(|_| "127.0.0.1:3030".to_string());
    let mut data_dir = env::var("AUTOMATAFL_DATA_DIR").ok();

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" | "-b" => {
                if let Some(val) = args.next() {
                    bind_addr = val;
                }
            }
            "--data-dir" | "-d" => {
                if let Some(val) = args.next() {
                    data_dir = Some(val);
                }
            }
            "--help" | "-h" => {
                println!(
                    "Usage: automatafl-server [--bind 127.0.0.1:3030] [--data-dir ./data]"
                );
                return Ok(());
            }
            _ => {}
        }
    }

    let store = match data_dir {
        Some(ref dir) => {
            println!("Using persistent game store at {dir}");
            GameStore::disk(dir)?
        }
        None => GameStore::memory(),
    };

    let state = AppState::new(store)?;
    let app = build_router(state);

    let addr: SocketAddr = bind_addr.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Automatafl fog-of-war server listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
