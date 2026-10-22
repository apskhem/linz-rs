//! A starter project for a linz-rs service: a small task tracker with authenticated writes,
//! validation, pagination, file attachments, tests and generated OpenAPI docs.
//!
//! Copy this directory into a new crate's `src/` to start a project. The layout:
//!
//! - `main.rs`: the entry point; reads the config and serves the app.
//! - `app.rs`: assembles the routes, docs, CORS and logging into an `axum::Router`.
//! - `config.rs`: settings from environment variables.
//! - `state.rs`: the router state shared by all handlers (`ctx.state`).
//! - `auth.rs`: the bearer token security scheme.
//! - `error.rs`: domain errors and their HTTP rendering.
//! - `store.rs`: the domain models and an in-memory store to replace with a database.
//! - `routes/`: one module per resource, each with its DTOs, endpoints and handlers.
//! - `tests.rs`: end-to-end tests that call the router without a network.
//!
//! Run it with `cargo run --example bootstrap` and open <http://localhost:3000/docs>.
//! Print the OpenAPI document with `cargo run --example bootstrap -- openapi`.
//!
//! ```console
//! $ curl -s localhost:3000/v1/tasks -H 'authorization: Bearer dev-token' \
//!     -H 'content-type: application/json' -d '{"title":"Write docs","tags":["docs"]}'
//! $ curl -s 'localhost:3000/v1/tasks?tag=docs'
//! $ curl -s localhost:3000/v1/tasks/1/attachments -H 'authorization: Bearer dev-token' -F file=@README.md
//! $ curl -s localhost:3000/v1/tasks/1/attachments/2 -o README.copy.md
//! ```

mod app;
mod auth;
mod config;
mod error;
mod routes;
mod state;
mod store;
#[cfg(test)]
mod tests;

use linz_rs::build_json;

use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;

    if std::env::args().nth(1).as_deref() == Some("openapi") {
        let document = build_json(&app::spec(), app::api(&config).endpoints())?;
        println!("{}", serde_json::to_string_pretty(&document)?);
        return Ok(());
    }

    let addr = config.addr;
    let app = app::build(AppState::new(config))?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Listening on http://{addr} (docs at {})", app::DOCS_PATH);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Resolves on Ctrl+C, or on SIGTERM (sent by Docker and Kubernetes) on Unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    println!("Shutting down");
}
