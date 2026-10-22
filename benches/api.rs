//! Benchmarks of request handling, OpenAPI document building and route registration.
//!
//! Run with `cargo bench`; pass a filter to run a subset, e.g. `cargo bench -- request/`.

use std::hint::black_box;

use axum::Router;
use axum::body::{
    Body,
    Bytes,
};
use criterion::{
    BenchmarkId,
    Criterion,
    Throughput,
    criterion_group,
    criterion_main,
};
use http::{
    Request,
    StatusCode,
    header,
};
use http_body_util::BodyExt;
use linz_rs::*;
use tokio::runtime::Runtime;
use tower::ServiceExt;

mod common;

use common::{
    app,
    docs,
    plain_axum_app,
    users,
};

/// An API with `n` copies of the users resource, nested under `/r{i}`.
fn api(n: usize) -> Api {
    (0..n).fold(Api::new(), |api, i| api.nest(&format!("/r{i}"), users(&i.to_string())))
}

const NEW_USER: &str = r#"{"username":"alice","age":30,"email":"alice@example.com"}"#;
const INVALID_USER: &str = r#"{"username":"al","age":12,"email":"not-an-email"}"#;
const BOUNDARY: &str = "XyZ";

struct Case {
    name: &'static str,
    status: StatusCode,
    request: fn() -> Request<Body>,
}

const CASES: &[Case] = &[
    Case {
        name: "get_path_param",
        status: StatusCode::OK,
        request: || Request::get("/users/42").body(Body::empty()).unwrap(),
    },
    Case {
        name: "get_query_list_10",
        status: StatusCode::OK,
        request: || Request::get("/users?limit=10&offset=0").body(Body::empty()).unwrap(),
    },
    Case {
        name: "get_query_list_100",
        status: StatusCode::OK,
        request: || Request::get("/users?limit=100&offset=0").body(Body::empty()).unwrap(),
    },
    Case {
        name: "post_json",
        status: StatusCode::CREATED,
        request: || json_request(NEW_USER),
    },
    Case {
        name: "post_json_invalid",
        status: StatusCode::BAD_REQUEST,
        request: || json_request(INVALID_USER),
    },
    Case {
        name: "put_urlencoded",
        status: StatusCode::OK,
        request: || {
            Request::put("/users/1")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("username=alice&age=30&email=alice%40example.com"))
                .unwrap()
        },
    },
    Case {
        name: "post_multipart",
        status: StatusCode::CREATED,
        request: || {
            let body = format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n\
                 --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.bin\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n{}\r\n\
                 --{BOUNDARY}--\r\n",
                "x".repeat(1024)
            );
            Request::post("/uploads")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
                .unwrap()
        },
    },
    Case {
        name: "get_auth_cookie_header",
        status: StatusCode::OK,
        request: || {
            Request::get("/me")
                .header(header::AUTHORIZATION, "Bearer secret")
                .header(header::COOKIE, "session=abc")
                .header("x-client", "bench")
                .body(Body::empty())
                .unwrap()
        },
    },
    Case {
        name: "get_auth_rejected",
        status: StatusCode::UNAUTHORIZED,
        request: || {
            Request::get("/me")
                .header(header::AUTHORIZATION, "Bearer wrong")
                .body(Body::empty())
                .unwrap()
        },
    },
    Case {
        name: "not_found",
        status: StatusCode::NOT_FOUND,
        request: || Request::get("/nowhere").body(Body::empty()).unwrap(),
    },
    Case {
        name: "get_openapi_json",
        status: StatusCode::OK,
        request: || Request::get("/openapi.json").body(Body::empty()).unwrap(),
    },
];

fn json_request(body: &'static str) -> Request<Body> {
    Request::post("/users")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

/// Sends the request and reads the whole response body.
async fn send(app: Router, req: Request<Body>) -> (StatusCode, Bytes) {
    let response = app.oneshot(req).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, body)
}

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn request(c: &mut Criterion) {
    let rt = runtime();
    let app = app();
    let mut group = c.benchmark_group("request");
    group.throughput(Throughput::Elements(1));

    for case in CASES {
        // Also warms the schema cache, so the benchmark measures the steady state.
        let (status, body) = rt.block_on(send(app.clone(), (case.request)()));
        assert_eq!(status, case.status, "{}: {}", case.name, String::from_utf8_lossy(&body));

        group.bench_function(case.name, |b| {
            b.to_async(&rt)
                .iter(|| async { black_box(send(app.clone(), (case.request)()).await) });
        });
    }
    group.finish();
}

fn overhead(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("overhead");
    group.throughput(Throughput::Elements(1));

    for (name, app) in [("plain_axum", plain_axum_app()), ("linz_rs", app())] {
        let (status, _) = rt.block_on(send(app.clone(), json_request(NEW_USER)));
        assert_eq!(status, StatusCode::CREATED, "{name}");

        group.bench_function(name, |b| {
            b.to_async(&rt)
                .iter(|| async { black_box(send(app.clone(), json_request(NEW_USER)).await) });
        });
    }
    group.finish();
}

fn spec(c: &mut Criterion) {
    let config = BuilderConfig::new("Bench API", "1.0.0");
    let mut group = c.benchmark_group("build_json");

    for n in [1, 10, 50] {
        let endpoints = api(n).into_endpoints();
        group.throughput(Throughput::Elements(endpoints.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(endpoints.len()),
            &endpoints,
            |b, endpoints| {
                b.iter(|| build_json(&config, black_box(endpoints)).unwrap());
            },
        );
    }
    group.finish();
}

fn registration(c: &mut Criterion) {
    let mut group = c.benchmark_group("create_api");

    for n in [1, 10, 50] {
        let api = api(n);
        let len = api.endpoints().len();
        group.throughput(Throughput::Elements(len as u64));
        group.bench_with_input(BenchmarkId::from_parameter(len), &api, |b, api| {
            b.iter(|| {
                create_api(
                    Router::new(),
                    api.clone(),
                    CreateApiConfig {
                        docs: Some(docs()),
                        ..Default::default()
                    },
                )
                .unwrap()
            });
        });
    }
    group.finish();
}

criterion_group!(benches, request, overhead, spec, registration);
criterion_main!(benches);
