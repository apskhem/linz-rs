//! End-to-end tests: requests go through the whole router, without a network.
//! Run them with `cargo test --example bootstrap`.

use axum::Router;
use axum::body::Body;
use http::{
    Request,
    StatusCode,
    header,
};
use http_body_util::BodyExt;
use serde_json::{
    Value,
    json,
};
use tower::ServiceExt;

use crate::app;
use crate::config::Config;
use crate::state::AppState;

const TOKEN: &str = "test-token";

fn test_app() -> Router {
    app::build(AppState::new(Config {
        api_token: TOKEN.to_string(),
        max_attachment_bytes: 16,
        ..Config::default()
    }))
    .expect("valid routes")
}

struct TestResponse {
    status: StatusCode,
    headers: http::HeaderMap,
    body: bytes::Bytes,
}

impl TestResponse {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("a JSON body")
    }
}

async fn send(app: &Router, request: Request<Body>) -> TestResponse {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    TestResponse { status, headers, body }
}

fn json_request(method: &str, uri: &str) -> http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
}

async fn create_task(app: &Router, body: Value) -> TestResponse {
    let request = json_request("POST", "/v1/tasks")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    send(app, request).await
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn health() {
    let response = send(&test_app(), get("/health")).await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json()["status"], "ok");
}

#[tokio::test]
async fn serves_the_docs() {
    let app = test_app();

    let spec = send(&app, get(app::SPEC_PATH)).await;
    assert_eq!(spec.status, StatusCode::OK);
    assert!(spec.json()["paths"]["/v1/tasks/{taskId}"]["patch"].is_object());

    assert_eq!(send(&app, get(app::DOCS_PATH)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn creates_and_reads_a_task() {
    let app = test_app();

    let created = create_task(&app, json!({ "title": "Write docs", "tags": ["docs"] })).await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.headers[header::LOCATION], "/v1/tasks/1");
    assert_eq!(
        created.json(),
        json!({
            "id": 1,
            "title": "Write docs",
            "description": null,
            "status": "todo",
            "tags": ["docs"],
            "createdBy": "admin",
            "attachments": [],
        })
    );

    let fetched = send(&app, get("/v1/tasks/1")).await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.json(), created.json());
}

#[tokio::test]
async fn writes_require_a_valid_token() {
    let app = test_app();
    let body = json!({ "title": "Write docs" }).to_string();

    let missing = json_request("POST", "/v1/tasks")
        .body(Body::from(body.clone()))
        .unwrap();
    assert_eq!(send(&app, missing).await.status, StatusCode::UNAUTHORIZED);

    let wrong = json_request("POST", "/v1/tasks")
        .header(header::AUTHORIZATION, "Bearer nope")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(send(&app, wrong).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_invalid_tasks() {
    let response = create_task(&test_app(), json!({ "title": "   ", "status": "blocked" })).await;

    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    let errors = &response.json()["errors"][0]["meta"];
    assert_eq!(errors["in"], "body");
    assert_eq!(errors["result"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn rejects_duplicate_titles() {
    let app = test_app();
    create_task(&app, json!({ "title": "Write docs" })).await;

    let response = create_task(&app, json!({ "title": "write DOCS" })).await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.json()["errors"][0]["code"], "title_taken");
    assert_eq!(response.json()["errors"][0]["source"]["pointer"], "/title");
}

#[tokio::test]
async fn filters_and_paginates() {
    let app = test_app();
    for (title, tag) in [("a", "x"), ("b", "y"), ("c", "x"), ("d", "x")] {
        create_task(&app, json!({ "title": title, "tags": [tag] })).await;
    }

    let page = send(&app, get("/v1/tasks?tag=x&offset=1&limit=1")).await.json();
    assert_eq!(page["total"], 3);
    assert_eq!(page["items"][0]["title"], "c");

    let invalid = send(&app, get("/v1/tasks?limit=500")).await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid.json()["errors"][0]["meta"]["in"], "queries");
}

#[tokio::test]
async fn updates_and_deletes_a_task() {
    let app = test_app();
    create_task(&app, json!({ "title": "Write docs" })).await;

    let patch = json_request("PATCH", "/v1/tasks/1")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::from(json!({ "status": "done" }).to_string()))
        .unwrap();
    let updated = send(&app, patch).await;
    assert_eq!(updated.status, StatusCode::OK);
    assert_eq!(updated.json()["status"], "done");
    assert_eq!(updated.json()["title"], "Write docs");

    let delete = Request::delete("/v1/tasks/1")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let deleted = send(&app, delete).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert!(deleted.body.is_empty());

    assert_eq!(send(&app, get("/v1/tasks/1")).await.status, StatusCode::NOT_FOUND);
}

fn upload(uri: &str, file_name: &str, data: &str) -> Request<Body> {
    let body = format!(
        "--BOUNDARY\r\n\
         content-disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n\
         content-type: text/plain\r\n\r\n\
         {data}\r\n\
         --BOUNDARY--\r\n"
    );
    Request::post(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "multipart/form-data; boundary=BOUNDARY")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn uploads_and_downloads_attachments() {
    let app = test_app();
    create_task(&app, json!({ "title": "Write docs" })).await;

    let uploaded = send(
        &app,
        upload("/v1/tasks/1/attachments", "C:\\docs\\my notes.txt", "hello"),
    )
    .await;
    assert_eq!(uploaded.status, StatusCode::CREATED);
    let info = uploaded.json();
    assert_eq!(info["fileName"], "my_notes.txt");
    assert_eq!(info["size"], 5);

    let downloaded = send(&app, get(&format!("/v1/tasks/1/attachments/{}", info["id"]))).await;
    assert_eq!(downloaded.status, StatusCode::OK);
    assert_eq!(downloaded.headers[header::CONTENT_TYPE], "text/plain");
    assert_eq!(
        downloaded.headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"my_notes.txt\""
    );
    assert_eq!(&downloaded.body[..], b"hello");

    let task = send(&app, get("/v1/tasks/1")).await.json();
    assert_eq!(task["attachments"][0], info);
}

#[tokio::test]
async fn rejects_large_attachments() {
    let app = test_app();
    create_task(&app, json!({ "title": "Write docs" })).await;

    let response = send(&app, upload("/v1/tasks/1/attachments", "big.txt", &"x".repeat(17))).await;
    assert_eq!(response.status, StatusCode::PAYLOAD_TOO_LARGE);

    let missing_task = send(&app, upload("/v1/tasks/9/attachments", "a.txt", "a")).await;
    assert_eq!(missing_task.status, StatusCode::NOT_FOUND);
}
