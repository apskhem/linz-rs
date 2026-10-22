//! Assembles the application: routes, docs, CORS and logging.

use std::sync::Arc;

use axum::Router;
use linz_rs::*;

use crate::config::Config;
use crate::state::AppState;
use crate::{
    auth,
    routes,
};

pub const DOCS_PATH: &str = "/docs";
pub const SPEC_PATH: &str = "/openapi.json";

/// The metadata of the OpenAPI document.
pub fn spec() -> BuilderConfig {
    let mut spec = BuilderConfig::new("Task Tracker API", env!("CARGO_PKG_VERSION"));
    spec.info.description =
        Some("A small task tracker built with linz-rs. Reads are public; writes need a bearer token.".to_string());
    spec
}

/// Every endpoint, with the security schemes that the configuration requires.
pub fn api(config: &Config) -> Api<AppState> {
    routes::api(&auth::bearer(config.api_token.clone()))
}

/// Builds the router, ready to serve or to test with `tower::ServiceExt::oneshot`.
pub fn build(state: AppState) -> Result<Router, LinzError> {
    let app = create_api(
        Router::new(),
        api(&state.config),
        CreateApiConfig {
            cors: Some(CorsConfig::Permissive),
            docs: Some(DocsConfig {
                viewer: DocsViewer::Scalar,
                spec: spec(),
                docs_path: DOCS_PATH.to_string(),
                spec_path: SPEC_PATH.to_string(),
                theme: None,
            }),
            logger: Some(Arc::new(StdoutLogger)),
            ..Default::default()
        },
    )?;

    Ok(app.with_state(state))
}

/// Prints linz-rs logs. Replace with `tracing` in a real service (see MANUAL.md, 13.13).
struct StdoutLogger;

impl Logger for StdoutLogger {
    fn error(&self, message: &str) {
        eprintln!("[error] {message}");
    }

    fn warning(&self, message: &str) {
        eprintln!("[warn] {message}");
    }

    fn info(&self, message: &str) {
        println!("[info] {message}");
    }
}
