//! Benchmarks of request handling, OpenAPI document building and route registration.
//!
//! Run with `cargo bench`; pass a filter to run a subset, e.g. `cargo bench -- request/`.

use std::hint::black_box;

use axum::Router;
use axum::body::{
    Body,
    Bytes,
};
use axum::routing::post;
use criterion::{
    BenchmarkId,
    Criterion,
    Throughput,
    criterion_group,
    criterion_main,
};
use http::{
    Extensions,
    Request,
    StatusCode,
    header,
};
use http_body_util::BodyExt;
use linz_rs::security::AuthenticationScheme;
use linz_rs::*;
use schemars::JsonSchema;
use serde::{
    Deserialize,
    Serialize,
};
use tokio::runtime::Runtime;
use tower::ServiceExt;

#[derive(Deserialize, JsonSchema)]
struct UserPath {
    user_id: u32,
}

#[derive(Deserialize, JsonSchema)]
struct ListQuery {
    #[schemars(range(min = 1, max = 100))]
    limit: u32,
    offset: u32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct NewUser {
    #[schemars(length(min = 3, max = 20))]
    username: String,
    #[schemars(range(min = 18, max = 99))]
    age: u32,
    #[schemars(email)]
    email: String,
}

#[derive(Serialize, JsonSchema)]
struct User {
    id: u32,
    username: String,
    age: u32,
    email: String,
}

#[derive(Deserialize, JsonSchema)]
struct Upload {
    file: File,
    field: String,
}

#[derive(Serialize, JsonSchema)]
struct UploadResult {
    field: String,
    size: usize,
}

#[derive(Deserialize, JsonSchema)]
struct Session {
    session: String,
}

#[derive(Deserialize, JsonSchema)]
struct ClientHeader {
    #[serde(rename = "X-Client")]
    client: String,
}

#[derive(Serialize, JsonSchema)]
struct Me {
    session: String,
    client: String,
}

fn user(id: u32) -> User {
    User {
        id,
        username: format!("user{id}"),
        age: 30,
        email: format!("user{id}@example.com"),
    }
}

fn bearer() -> Security {
    Security::new(
        "bearerAuth",
        SecurityType::Http {
            scheme: AuthenticationScheme::Bearer,
            bearer_format: None,
        },
        |credentials: Credentials| {
            async move {
                match credentials.token.as_deref() {
                    Some("secret") => Ok(Extensions::new()),
                    _ => Err(ApiError::detail(StatusCode::UNAUTHORIZED, "Invalid token")),
                }
            }
        },
    )
}

/// A CRUD resource; `suffix` keeps the operation ids unique when it is nested many times.
fn users(suffix: &str) -> Api {
    Api::new()
        .get(
            "/users",
            Endpoint::new(format!("listUsers{suffix}"))
                .query::<ListQuery>()
                .returns(200, JsonBody::new::<Vec<User>>()),
            |req, _| {
                async move {
                    Ok((req.queries.offset..req.queries.offset + req.queries.limit)
                        .map(user)
                        .collect::<Vec<_>>())
                }
            },
        )
        .get(
            "/users/:user_id",
            Endpoint::new(format!("getUser{suffix}"))
                .path::<UserPath>()
                .returns(200, JsonBody::new::<User>())
                .response(404, true),
            |req, _| async move { Ok(user(req.params.user_id)) },
        )
        .post(
            "/users",
            Endpoint::new(format!("createUser{suffix}"))
                .request_body(JsonBody::new::<NewUser>())
                .returns(201, JsonBody::new::<User>()),
            |req, _| {
                async move {
                    Ok(User {
                        id: 1,
                        username: req.body.username,
                        age: req.body.age,
                        email: req.body.email,
                    })
                }
            },
        )
        .put(
            "/users/:user_id",
            Endpoint::new(format!("replaceUser{suffix}"))
                .path::<UserPath>()
                .request_body(UrlEncodedBody::new::<NewUser>())
                .returns(200, JsonBody::new::<User>()),
            |req, _| {
                async move {
                    Ok(User {
                        id: req.params.user_id,
                        username: req.body.username,
                        age: req.body.age,
                        email: req.body.email,
                    })
                }
            },
        )
        .post(
            "/uploads",
            Endpoint::new(format!("upload{suffix}"))
                .request_body(FormDataBody::new::<Upload>())
                .returns(201, JsonBody::new::<UploadResult>()),
            |req, _| {
                async move {
                    Ok(UploadResult {
                        field: req.body.field,
                        size: req.body.file.data.len(),
                    })
                }
            },
        )
        .get(
            "/me",
            Endpoint::new(format!("getMe{suffix}"))
                .security(bearer().apply(Vec::<String>::new()))
                .cookie::<Session>()
                .header::<ClientHeader>()
                .returns(200, JsonBody::new::<Me>()),
            |req, _| {
                async move {
                    Ok(Me {
                        session: req.cookies.session,
                        client: req.headers.client,
                    })
                }
            },
        )
        .config(GroupConfig::new().tag("Users"))
}

/// An API with `n` copies of the users resource, nested under `/r{i}`.
fn api(n: usize) -> Api {
    (0..n).fold(Api::new(), |api, i| api.nest(&format!("/r{i}"), users(&i.to_string())))
}

fn docs() -> DocsConfig {
    DocsConfig {
        viewer: DocsViewer::Scalar,
        spec: BuilderConfig::new("Bench API", "1.0.0"),
        docs_path: "/docs".into(),
        spec_path: "/openapi.json".into(),
        theme: None,
    }
}

fn app() -> Router {
    create_api(
        Router::new(),
        users(""),
        CreateApiConfig {
            docs: Some(docs()),
            ..Default::default()
        },
    )
    .expect("valid routes")
}

/// The `createUser` endpoint written with plain axum: the same JSON in and out, without
/// validation. The difference to `request/post_json` is the overhead of linz-rs.
fn plain_axum_app() -> Router {
    Router::new().route(
        "/users",
        post(|body: Bytes| {
            async move {
                let Ok(new_user) = serde_json::from_slice::<NewUser>(&body) else {
                    return (
                        StatusCode::BAD_REQUEST,
                        [(header::CONTENT_TYPE, "application/json")],
                        Vec::new(),
                    );
                };
                let user = User {
                    id: 1,
                    username: new_user.username,
                    age: new_user.age,
                    email: new_user.email,
                };
                (
                    StatusCode::CREATED,
                    [(header::CONTENT_TYPE, "application/json")],
                    serde_json::to_vec(&user).unwrap(),
                )
            }
        }),
    )
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
