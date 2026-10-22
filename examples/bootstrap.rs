//! A port of linz's `examples/bootstrap`.
//!
//! Run with `cargo run --example bootstrap` and open http://localhost:3000.

use std::sync::Arc;

use axum::Router;
use http::StatusCode;
use linz_rs::*;
use schemars::JsonSchema;
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Deserialize, JsonSchema)]
struct EchoPath {
    path: String,
}

#[derive(Serialize, JsonSchema)]
struct EchoPathResult {
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct EchoBody {
    echo: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct BodyResult {
    body: String,
}

#[derive(Deserialize, JsonSchema)]
struct EchoQuery {
    echo: String,
}

#[derive(Deserialize, JsonSchema)]
struct Upload {
    file: File,
    field: String,
}

#[derive(Serialize, JsonSchema)]
struct UploadResult {
    field: String,
}

#[derive(Deserialize, JsonSchema)]
struct UploadFileHeaders {
    /// This is a custom header
    #[serde(rename = "X-Custom-Header")]
    _custom_header: String,
}

fn simple_controller() -> LinzEndpointGroup {
    let endpoints = vec![
        (
            "post:/echo/:path".to_string(),
            Endpoint::new("echoPathSimple")
                .path::<EchoPath>()
                .returns(201, JsonBody::new::<EchoPathResult>())
                .response(404, true)
                .handler(|req, _| async move { Ok(EchoPathResult { path: req.params.path }) }),
        ),
        (
            "post:/echo/simple/body".to_string(),
            Endpoint::new("echoBodySimple")
                .request_body(JsonBody::new::<EchoBody>())
                .returns(201, JsonBody::new::<BodyResult>())
                .response(404, true)
                .handler(|req, _| {
                    async move {
                        Ok(BodyResult {
                            body: req.body.echo.unwrap_or_else(|| "null!".to_string()),
                        })
                    }
                }),
        ),
        (
            "post:/echo/simple/query".to_string(),
            Endpoint::new("echoBodyQuery")
                .query::<EchoQuery>()
                .returns(201, JsonBody::new::<BodyResult>())
                .response(404, true)
                .handler(|req, _| async move { Ok(BodyResult { body: req.queries.echo }) }),
        ),
        (
            "post:/echo/simple/void".to_string(),
            Endpoint::new("echoVoidSimple")
                .request_body(JsonBody::new::<EchoBody>().describe("Echo void result"))
                .returns(201, JsonBody::new::<()>())
                .response(404, true)
                .handler(|_, _| async move { Ok(()) }),
        ),
        (
            "post:/echo/simple/error".to_string(),
            Endpoint::new("echoError")
                .returns(201, JsonBody::new::<serde_json::Value>())
                .response(404, true)
                .handler(|_, _| {
                    async move {
                        Err::<serde_json::Value, _>(ApiError::new(
                            StatusCode::NOT_FOUND,
                            JsonApiErrorItem::detail("Not Found"),
                        ))
                    }
                }),
        ),
        (
            "post:/echo/simple/upload".to_string(),
            Endpoint::new("echoUploadSimple")
                .request_body(
                    FormDataBody::new::<Upload>().encoding(
                        "file",
                        EncodingItem::new()
                            .content_type(["image/png", "image/jpeg"])
                            .headers::<UploadFileHeaders>(),
                    ),
                )
                .returns(201, JsonBody::new::<UploadResult>())
                .response(404, true)
                .handler(|req, _| {
                    async move {
                        let file = req.body.file;
                        let name = file.name.unwrap_or_else(|| "upload.bin".to_string());

                        std::fs::write(&name, &file.data)
                            .map_err(|e| ApiError::detail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                        Ok(UploadResult { field: req.body.field })
                    }
                }),
        ),
    ];

    apply_group_config(
        endpoints,
        GroupConfig {
            tags: vec![oas::Tag {
                name: "Simple".to_string(),
                description: None,
                extensions: Default::default(),
            }],
            ..Default::default()
        },
    )
}

struct StdoutLogger;

impl Logger for StdoutLogger {
    fn info(&self, message: &str) {
        println!("[info] {message}");
    }

    fn error(&self, message: &str) {
        eprintln!("[error] {message}");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let router = merge_endpoint_groups("/v1", vec![simple_controller()])?;

    let app = create_api(
        Router::new(),
        router,
        CreateApiConfig {
            cors: Some(CorsConfig::Permissive),
            docs: Some(DocsConfig {
                viewer: DocsViewer::Scalar,
                spec: BuilderConfig::new("Linz Bootstrap API", env!("CARGO_PKG_VERSION")),
                docs_path: "/".to_string(),
                spec_path: "/openapi.json".to_string(),
                theme: None,
            }),
            logger: Some(Arc::new(StdoutLogger)),
            ..Default::default()
        },
    )?;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
