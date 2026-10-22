//! The API served by the benchmarks in `api.rs` and by the `bench_server` example, which
//! `load.sh` load-tests.

use axum::Router;
use axum::body::Bytes;
use axum::extract::Path;
use axum::routing::{
    get,
    post,
};
use http::{
    Extensions,
    StatusCode,
    header,
};
use linz_rs::security::AuthenticationScheme;
use linz_rs::*;
use schemars::JsonSchema;
use serde::{
    Deserialize,
    Serialize,
};

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
pub fn users(suffix: &str) -> Api {
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

pub fn docs() -> DocsConfig {
    DocsConfig {
        viewer: DocsViewer::Scalar,
        spec: BuilderConfig::new("Bench API", "1.0.0"),
        docs_path: "/docs".into(),
        spec_path: "/openapi.json".into(),
        theme: None,
    }
}

pub fn app() -> Router {
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

/// The `getUser` and `createUser` endpoints written with plain axum: the same JSON in and
/// out, without validation. The difference to linz-rs is its overhead.
pub fn plain_axum_app() -> Router {
    Router::new()
        .route(
            "/users/{user_id}",
            get(|Path(user_id): Path<u32>| {
                async move {
                    (
                        [(header::CONTENT_TYPE, "application/json")],
                        serde_json::to_vec(&user(user_id)).unwrap(),
                    )
                }
            }),
        )
        .route(
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
