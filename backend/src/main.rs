mod accounts;
mod admin;
mod auth;
mod common;
mod config;
mod db;
mod game;
mod html;
mod matchmaking;
mod middleware;
mod repositories;
mod server;
mod services;
mod transactions;
mod validation;
mod web;

const CARGO_PACKAGE_VERSION: Option<&str> = std::option_env!("CARGO_PACKAGE_VERSION");

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() {
    // Initialize structured logging early
    server::init_tracing();

    // Run the server
    if let Err(e) = server::run().await {
        tracing::error!("Server failed: {}", e);
        std::process::exit(1);
    }
}
