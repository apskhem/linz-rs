use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::response::Response;
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
use serde_json::{
    Value,
    json,
};
use tower::ServiceExt;

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
    #[allow(dead_code)]
    coords: Option<(f64, f64)>,
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
struct User {
    #[schemars(length(min = 3, max = 20))]
    username: String,
    #[schemars(range(min = 18, max = 99))]
    age: u32,
}

#[derive(Deserialize, JsonSchema)]
struct UserPath {
    user_id: u32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct UserResult {
    id: u32,
    username: String,
    age: u32,
}

#[derive(Deserialize, JsonSchema)]
struct Upload {
    file: File,
    field: String,
}

#[derive(Serialize, JsonSchema)]
struct UploadResult {
    field: String,
    filename: Option<String>,
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

#[derive(Clone)]
struct UserId(String);

#[derive(Serialize, JsonSchema)]
struct Bounded {
    #[schemars(range(max = 10))]
    n: u32,
}

#[derive(Deserialize, JsonSchema)]
struct Subscribe {
    #[schemars(email)]
    #[allow(dead_code)]
    email: String,
}

#[derive(Clone)]
struct AppState {
    greeting: String,
}

#[derive(Serialize, JsonSchema)]
struct Greeting {
    message: String,
}

fn bearer() -> Security {
    Security::new(
        "bearerAuth",
        SecurityType::Http {
            scheme: AuthenticationScheme::Bearer,
            bearer_format: Some("JWT".into()),
        },
        |credentials: Credentials| {
            async move {
                match credentials.token.as_deref() {
                    Some("secret") => {
                        let mut extensions = Extensions::new();
                        extensions.insert(UserId("u-1".into()));
                        Ok(extensions)
                    }
                    _ => Err(ApiError::detail(StatusCode::UNAUTHORIZED, "Invalid token")),
                }
            }
        },
    )
}

fn endpoints() -> LinzEndpointGroup {
    vec![
        (
            "post:/echo/:path".into(),
            Endpoint::new("echoPathSimple")
                .path::<EchoPath>()
                .returns(201, JsonBody::new::<EchoPathResult>())
                .response(404, true)
                .handler(|req, _| async move { Ok(EchoPathResult { path: req.params.path }) }),
        ),
        (
            "post:/echo/simple/body".into(),
            Endpoint::new("echoBodySimple")
                .request_body(JsonBody::new::<EchoBody>())
                .returns(201, JsonBody::new::<BodyResult>())
                .handler(|req, _| {
                    async move {
                        Ok(BodyResult {
                            body: req.body.echo.unwrap_or_else(|| "null!".into()),
                        })
                    }
                }),
        ),
        (
            "post:/echo/simple/query".into(),
            Endpoint::new("echoBodyQuery")
                .query::<EchoQuery>()
                .returns(201, JsonBody::new::<BodyResult>())
                .handler(|req, _| async move { Ok(BodyResult { body: req.queries.echo }) }),
        ),
        (
            "post:/echo/simple/void".into(),
            Endpoint::new("echoVoidSimple")
                .request_body(JsonBody::new::<EchoBody>().describe("Echo void result"))
                .returns(201, JsonBody::new::<()>())
                .handler(|_, _| async move { Ok(()) }),
        ),
        (
            "post:/echo/simple/error".into(),
            Endpoint::new("echoError")
                .returns(201, JsonBody::new::<BodyResult>())
                .response(404, true)
                .handler(
                    |_, _| async move { Err::<BodyResult, _>(ApiError::detail(StatusCode::NOT_FOUND, "Not Found")) },
                ),
        ),
        (
            "post:/echo/simple/upload".into(),
            Endpoint::new("echoUploadSimple")
                .request_body(
                    FormDataBody::new::<Upload>().encoding("file", EncodingItem::new().content_type(["image/png"])),
                )
                .returns(201, JsonBody::new::<UploadResult>())
                .handler(|req, _| {
                    async move {
                        Ok(UploadResult {
                            field: req.body.field,
                            filename: req.body.file.name,
                            size: req.body.file.data.len(),
                        })
                    }
                }),
        ),
        (
            "put:/users/{user_id}".into(),
            Endpoint::new("updateUser")
                .path::<UserPath>()
                .request_body(UrlEncodedBody::new::<User>())
                .returns(200, JsonBody::new::<UserResult>())
                .handler(|req, _| {
                    async move {
                        Ok(UserResult {
                            id: req.params.user_id,
                            username: req.body.username,
                            age: req.body.age,
                        })
                    }
                }),
        ),
        (
            "get:/me".into(),
            Endpoint::new("getMe")
                .security(bearer().apply(["read"]))
                .cookie::<Session>()
                .header::<ClientHeader>()
                .returns(200, JsonBody::new::<Value>())
                .handler(|req, ctx| {
                    async move {
                        let user = ctx.extension::<UserId>()?.0.clone();
                        Ok(json!({
                            "user": user,
                            "session": req.cookies.session,
                            "client": req.headers.client,
                            "scopes": ctx.security[0].scopes,
                        }))
                    }
                }),
        ),
        (
            "get:/broken".into(),
            Endpoint::new("broken")
                .returns(200, JsonBody::new::<Bounded>())
                .handler(|_, _| async move { Ok(Bounded { n: 50 }) }),
        ),
        (
            "get:/undeclared".into(),
            Endpoint::new("undeclared")
                .returns(200, JsonBody::new::<Value>())
                .handler(|_, _| async move { Ok(HttpResponse::with_status(StatusCode::ACCEPTED, json!({}))) }),
        ),
        (
            "get:/teapot".into(),
            Endpoint::new("teapot")
                .returns_default(TextBody::new())
                .handler(|_, _| {
                    async move {
                        Ok(
                            HttpResponse::with_status(StatusCode::IM_A_TEAPOT, "short and stout".to_string())
                                .header(header::CACHE_CONTROL, "no-store".parse().unwrap()),
                        )
                    }
                }),
        ),
        (
            "post:/binary".into(),
            Endpoint::new("binary")
                .request_body(OctetStreamBody::new())
                .returns(201, OctetStreamBody::new())
                .handler(|req, _| {
                    async move {
                        let mut data = req.body.data.to_vec();
                        data.reverse();
                        Ok(File::new(data))
                    }
                }),
        ),
        (
            "get:/hidden".into(),
            Endpoint::new("hidden")
                .hidden(true)
                .returns(200, TextBody::new())
                .handler(|_, _| async move { Ok("hidden".to_string()) }),
        ),
        (
            "post:/subscribe".into(),
            Endpoint::new("subscribe")
                .request_body(JsonBody::new::<Subscribe>())
                .returns(204, JsonBody::new::<()>())
                .handler(|_, _| async move { Ok(HttpResponse::without_body(StatusCode::NO_CONTENT, None)) }),
        ),
    ]
}

fn spec_config() -> BuilderConfig {
    BuilderConfig::new("Linz Test API", "1.0.0")
}

fn spec() -> Value {
    build_json(&spec_config(), &endpoints()).unwrap()
}

fn app() -> Router {
    create_api(
        Router::new(),
        endpoints(),
        CreateApiConfig {
            cors: Some(CorsConfig::Permissive),
            docs: Some(DocsConfig {
                viewer: DocsViewer::Scalar,
                spec: spec_config(),
                docs_path: "/docs".into(),
                spec_path: "/openapi.json".into(),
                theme: None,
            }),
            ..Default::default()
        },
    )
    .unwrap()
}

async fn send(app: Router, req: Request<Body>) -> (StatusCode, http::HeaderMap, Vec<u8>) {
    let response: Response = app.oneshot(req).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, headers, body)
}

async fn send_json(app: Router, req: Request<Body>) -> (StatusCode, Value) {
    let (status, _, body) = send(app, req).await;
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    (status, body)
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::post(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn echoes_path_params() {
    let (status, body) = send_json(app(), post_json("/echo/hello", json!({}))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body, json!({ "path": "hello" }));
}

#[tokio::test]
async fn echoes_json_bodies() {
    let (status, body) = send_json(app(), post_json("/echo/simple/body", json!({ "echo": "hi" }))).await;
    assert_eq!((status, body), (StatusCode::CREATED, json!({ "body": "hi" })));

    let (status, body) = send_json(app(), post_json("/echo/simple/body", json!({ "echo": null }))).await;
    assert_eq!((status, body), (StatusCode::CREATED, json!({ "body": "null!" })));
}

#[tokio::test]
async fn rejects_invalid_payloads() {
    let (status, body) = send_json(app(), post_json("/echo/simple/body", json!({ "echo": 1 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["errors"][0]["meta"]["in"], "body");
    assert_eq!(body["errors"][0]["meta"]["result"][0]["path"], json!(["echo"]));

    // missing body
    let req = Request::post("/echo/simple/body").body(Body::empty()).unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["errors"][0]["meta"]["in"], "body");

    // malformed JSON
    let req = Request::post("/echo/simple/body")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{"))
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["errors"][0]["detail"].is_string());

    // unsupported content type
    let req = Request::post("/echo/simple/body")
        .header(header::CONTENT_TYPE, "text/csv")
        .body(Body::from("a,b"))
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(body["errors"][0]["detail"], "'text/csv' content type is not supported");
}

#[tokio::test]
async fn uses_last_occurrence_of_repeated_values() {
    let req = Request::post("/echo/simple/query?echo=a&echo=b")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!((status, body), (StatusCode::CREATED, json!({ "body": "b" })));

    let req = Request::put("/users/1")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("username=alice&age=20&username=bobby"))
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["username"], "bobby");
}

#[tokio::test]
async fn validates_formats() {
    let (status, body) = send_json(app(), post_json("/subscribe", json!({ "email": "not-an-email" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["errors"][0]["meta"]["result"][0]["path"], json!(["email"]));

    let (status, _, body) = send(app(), post_json("/subscribe", json!({ "email": "a@example.com" }))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
}

#[tokio::test]
async fn passes_state_to_handlers() {
    let endpoints: LinzEndpointGroup<AppState> = vec![(
        "get:/greet/:path".into(),
        Endpoint::new("greet")
            .path::<EchoPath>()
            .returns(200, JsonBody::new::<Greeting>())
            .handler(|req, ctx: Context<AppState>| {
                async move {
                    Ok(Greeting {
                        message: format!("{}, {}", ctx.state.greeting, req.params.path),
                    })
                }
            }),
    )];
    let app = create_api(Router::new(), endpoints, Default::default())
        .unwrap()
        .with_state(AppState {
            greeting: "hello".into(),
        });

    let (status, body) = send_json(app, Request::get("/greet/world").body(Body::empty()).unwrap()).await;
    assert_eq!((status, body), (StatusCode::OK, json!({ "message": "hello, world" })));
}

#[tokio::test]
async fn responds_without_body_for_void() {
    let (status, headers, body) = send(app(), post_json("/echo/simple/void", json!({ "echo": "x" }))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(body.is_empty());
    assert!(headers.get(header::CONTENT_TYPE).is_none());
}

#[tokio::test]
async fn renders_api_errors() {
    let req = Request::post("/echo/simple/error").body(Body::empty()).unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "errors": [{ "detail": "Not Found" }] }));
}

#[tokio::test]
async fn accepts_multipart_uploads() {
    let boundary = "XyZ";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\n\x01\x02\x03\r\n\
         --{boundary}--\r\n"
    );
    let req = Request::post("/echo/simple/upload")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body, json!({ "field": "value", "filename": "a.png", "size": 3 }));

    // a text field is not a file
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"\r\n\r\nx\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nv\r\n--{boundary}--\r\n"
    );
    let req = Request::post("/echo/simple/upload")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["errors"][0]["meta"]["in"], "body");
}

#[tokio::test]
async fn coerces_textual_inputs() {
    let req = Request::put("/users/42")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("username=alice&age=30"))
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "id": 42, "username": "alice", "age": 30 }));

    let req = Request::put("/users/abc")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("username=al&age=12"))
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let locations: Vec<&str> = body["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["meta"]["in"].as_str().unwrap())
        .collect();
    assert_eq!(locations, ["body", "params"]);
    assert_eq!(body["errors"][0]["meta"]["result"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn authenticates_with_security() {
    let req = Request::get("/me").body(Body::empty()).unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        body,
        json!({ "errors": [{ "detail": "Missing authorization header" }] })
    );

    let req = Request::get("/me")
        .header(header::AUTHORIZATION, "Bearer wrong")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "errors": [{ "detail": "Invalid token" }] }));

    let req = Request::get("/me")
        .header(header::AUTHORIZATION, "bearer secret")
        .header(header::COOKIE, "session=abc%20def; other=1")
        .header("x-client", "test")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({ "user": "u-1", "session": "abc def", "client": "test", "scopes": ["read"] })
    );
}

#[tokio::test]
async fn validates_responses() {
    let req = Request::get("/broken").body(Body::empty()).unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body, json!({ "errors": [{ "detail": "Internal server error" }] }));

    let req = Request::get("/undeclared").body(Body::empty()).unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body, json!({ "errors": [{ "detail": "Internal server error" }] }));
}

#[tokio::test]
async fn uses_default_response_and_custom_headers() {
    let req = Request::get("/teapot").body(Body::empty()).unwrap();
    let (status, headers, body) = send(app(), req).await;
    assert_eq!(status, StatusCode::IM_A_TEAPOT);
    assert_eq!(headers[header::CONTENT_TYPE], "text/plain");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(body, b"short and stout");
}

#[tokio::test]
async fn handles_octet_streams() {
    let req = Request::post("/binary")
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(vec![1u8, 2, 3]))
        .unwrap();
    let (status, headers, body) = send(app(), req).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers[header::CONTENT_TYPE], "application/octet-stream");
    assert_eq!(body, vec![3u8, 2, 1]);
}

#[tokio::test]
async fn responds_not_found() {
    let req = Request::get("/nope").body(Body::empty()).unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "errors": [{ "detail": "Cannot find GET /nope" }] }));

    // the path exists but not with this method
    let req = Request::delete("/echo/simple/body").body(Body::empty()).unwrap();
    let (status, body) = send_json(app(), req).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        json!({ "errors": [{ "detail": "Cannot find DELETE /echo/simple/body" }] })
    );

    // hidden endpoints are still accessible
    let req = Request::get("/hidden").body(Body::empty()).unwrap();
    let (status, _, body) = send(app(), req).await;
    assert_eq!((status, body), (StatusCode::OK, b"hidden".to_vec()));
}

#[tokio::test]
async fn calls_fallback_handler() {
    let app = create_api(
        Router::new(),
        vec![],
        CreateApiConfig {
            fallback_handler: Some(Arc::new(|req: axum::extract::Request| {
                Box::pin(async move { (req.uri().path() == "/health").then(|| Response::new(Body::from("ok"))) })
            })),
            ..Default::default()
        },
    )
    .unwrap();

    let (status, _, body) = send(app.clone(), Request::get("/health").body(Body::empty()).unwrap()).await;
    assert_eq!((status, body), (StatusCode::OK, b"ok".to_vec()));

    let (status, _) = send_json(app, Request::get("/other").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn applies_cors() {
    let req = Request::options("/echo/simple/body")
        .header(header::ORIGIN, "https://example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();
    let (status, headers, _) = send(app(), req).await;
    assert!(status.is_success());
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_HEADERS], "content-type");
}

#[tokio::test]
async fn serves_docs() {
    let (status, headers, body) = send(app(), Request::get("/openapi.json").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), spec());

    let (status, headers, body) = send(app(), Request::get("/docs").body(Body::empty()).unwrap()).await;
    let html = String::from_utf8(body).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "text/html");
    assert!(html.contains("<title>Linz Test API</title>"));
    assert!(html.contains("data-url=\"/openapi.json\""));
}

#[test]
fn builds_openapi_document() {
    let spec = spec();

    assert_eq!(spec["openapi"], "3.1.0");
    assert_eq!(
        spec["jsonSchemaDialect"],
        "https://spec.openapis.org/oas/3.1/dialect/base"
    );
    assert!(spec["paths"]["/hidden"].is_null());

    let echo = &spec["paths"]["/echo/{path}"]["post"];
    assert_eq!(echo["summary"], "echoPathSimple");
    assert_eq!(
        echo["parameters"],
        json!([{ "name": "path", "in": "path", "required": true, "schema": { "type": "string" } }])
    );
    assert_eq!(
        echo["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/EchoPathSimpleResponse"
    );
    assert_eq!(echo["responses"]["404"]["description"], "Not Found");
    assert_eq!(
        echo["responses"]["404"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/JsonApiError"
    );
    assert!(echo["responses"]["400"].is_object());
    assert!(echo["responses"]["401"].is_null());
    assert!(echo["responses"]["500"].is_object());

    let body = &spec["paths"]["/echo/simple/body"]["post"]["requestBody"];
    assert_eq!(body["required"], true);
    assert_eq!(
        body["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/EchoBodySimpleRequestBody"
    );

    let void = &spec["paths"]["/echo/simple/void"]["post"];
    assert_eq!(void["requestBody"]["description"], "Echo void result");
    assert_eq!(void["responses"]["201"]["content"], json!({ "application/json": {} }));

    let upload = &spec["paths"]["/echo/simple/upload"]["post"]["requestBody"]["content"]["multipart/form-data"];
    assert_eq!(upload["encoding"]["file"]["contentType"], "image/png");
    assert_eq!(
        spec["components"]["schemas"]["EchoUploadSimpleRequestBody"]["properties"]["file"],
        json!({ "type": "string", "format": "binary", "contentMediaType": "application/octet-stream" })
    );

    let me = &spec["paths"]["/me"]["get"];
    assert_eq!(me["security"], json!([{ "bearerAuth": ["read"] }]));
    assert!(me["responses"]["401"].is_object());
    assert_eq!(me["parameters"][0]["in"], "header");
    assert_eq!(me["parameters"][0]["name"], "X-Client");
    assert_eq!(me["parameters"][1]["in"], "cookie");
    assert_eq!(
        spec["components"]["securitySchemes"]["bearerAuth"],
        json!({ "type": "http", "scheme": "Bearer", "bearerFormat": "JWT" })
    );

    let coords = &spec["components"]["schemas"]["EchoBodySimpleRequestBody"]["properties"]["coords"];
    assert_eq!(coords["prefixItems"].as_array().unwrap().len(), 2);
    assert!(coords.get("items").is_none() || coords["items"] == json!(false));
    assert_eq!(
        spec["components"]["schemas"]["SubscribeRequestBody"]["properties"]["email"]["format"],
        "email"
    );

    let user_path = &spec["paths"]["/users/{user_id}"]["put"];
    assert_eq!(user_path["parameters"][0]["schema"]["type"], "integer");
    assert_eq!(
        spec["components"]["schemas"]["UpdateUserRequestBody"]["properties"]["username"]["minLength"],
        3
    );

    assert_eq!(
        spec["paths"]["/teapot"]["get"]["responses"]["default"]["content"]["text/plain"]["schema"]["$ref"],
        "#/components/schemas/TeapotResponse"
    );
    assert!(spec["components"]["schemas"]["JsonApiError"]["properties"]["errors"].is_object());
}

#[test]
fn groups_endpoints() {
    let group = apply_group_config(
        endpoints().into_iter().take(2).collect(),
        GroupConfig::new().tag("Simple").deprecated(true),
    );
    let merged = merge_endpoint_groups("/v1/", vec![group]).unwrap();

    let keys: Vec<&str> = merged.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["post:/v1/echo/:path", "post:/v1/echo/simple/body"]);
    assert_eq!(merged[0].1.meta().tags[0].name, "Simple");
    assert_eq!(merged[0].1.meta().deprecated, Some(true));

    let spec = build_json(&BuilderConfig::new("t", "1"), &merged).unwrap();
    assert_eq!(spec["paths"]["/v1/echo/{path}"]["post"]["tags"], json!(["Simple"]));
    assert_eq!(spec["paths"]["/v1/echo/{path}"]["post"]["deprecated"], true);
    assert_eq!(spec["tags"], json!([{ "name": "Simple" }]));

    let err = merge_endpoint_groups(
        "/v1",
        vec![
            endpoints().into_iter().take(1).collect(),
            endpoints().into_iter().take(1).collect(),
        ],
    )
    .unwrap_err();
    assert_eq!(
        err,
        LinzError::DuplicateKeys {
            keys: vec!["post:/v1/echo/:path".into()]
        }
    );
    assert_eq!(err.to_string(), "Duplicated keys occured: post:/v1/echo/:path");
}

#[test]
fn rejects_invalid_registrations() {
    let duplicated: LinzEndpointGroup = vec![
        endpoints().remove(0),
        ("get:/other/:path".into(), endpoints().remove(0).1),
    ];
    let err = create_api(Router::<()>::new(), duplicated, Default::default()).unwrap_err();
    assert!(matches!(err, LinzError::DuplicateOperationId { .. }));
    assert_eq!(
        err.to_string(),
        "Duplicate operation ID \"echoPathSimple\" for path /other/:path"
    );

    let invalid: LinzEndpointGroup = vec![("head:/x".into(), endpoints().remove(0).1)];
    assert_eq!(
        create_api(Router::<()>::new(), invalid, Default::default())
            .unwrap_err()
            .to_string(),
        "Invalid method \"head\" for path /x"
    );

    let route = |key: &str| -> LinzEndpointGroup { vec![(key.into(), endpoints().remove(0).1)] };
    let check = |key: &str| create_api(Router::<()>::new(), route(key), Default::default()).map(|_| ());

    // the path DTO `EchoPath` has the field `path`
    assert!(check("post:/a/:path").is_ok());
    assert!(check("post:/a/{path}").is_ok());
    assert_eq!(
        check("post:/a/:id").unwrap_err(),
        LinzError::UndeclaredPathParameter {
            operation_id: "echoPathSimple".into(),
            path: "/a/:id".into(),
            parameter: "id".into(),
            declared: vec!["path".into()],
        }
    );
    assert_eq!(
        check("post:/a/:path/:extra").unwrap_err().to_string(),
        "Path parameter \"extra\" of /a/:path/:extra (echoPathSimple) is not declared in its path type (declared: path)"
    );
    assert_eq!(
        check("post:/a").unwrap_err().to_string(),
        "The path type of /a (echoPathSimple) requires \"path\", which is not a parameter of the path"
    );
}

#[tokio::test]
async fn reports_missing_extensions() {
    let endpoints: LinzEndpointGroup = vec![(
        "get:/whoami".into(),
        Endpoint::new("whoami")
            .returns(200, TextBody::new())
            .handler(|_, ctx| async move { Ok(ctx.extension::<UserId>()?.0.clone()) }),
    )];
    let app = create_api(Router::new(), endpoints, Default::default()).unwrap();
    let (status, body) = send_json(app, Request::get("/whoami").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body,
        json!({ "errors": [{ "detail": "Missing request extension `api::UserId`" }] })
    );
}

#[test]
fn applies_security_without_scopes() {
    let scheme = bearer();
    let endpoints: LinzEndpointGroup = vec![(
        "get:/x".into(),
        Endpoint::new("x")
            .security(&scheme)
            .returns(200, TextBody::new())
            .handler(|_, _| async move { Ok(String::new()) }),
    )];
    let spec = build_json(&BuilderConfig::new("t", "1"), &endpoints).unwrap();
    assert_eq!(spec["paths"]["/x"]["get"]["security"], json!([{ "bearerAuth": [] }]));
}

#[test]
fn documents_all_securities_as_one_requirement() {
    let api_key = Security::new(
        "apiKey",
        SecurityType::ApiKey {
            name: "X-Api-Key".into(),
            r#in: linz_rs::security::In::Header,
        },
        |_: Credentials| async move { Ok(Extensions::new()) },
    );
    let endpoints: LinzEndpointGroup = vec![(
        "get:/x".into(),
        Endpoint::new("x")
            .security(bearer().apply(["read"]))
            .security(&api_key)
            .security(bearer().apply(["write", "read"]))
            .returns(200, TextBody::new())
            .handler(|_, _| async move { Ok(String::new()) }),
    )];
    let spec = build_json(&BuilderConfig::new("t", "1"), &endpoints).unwrap();

    // all of them are required at runtime: one requirement object (AND), not one per scheme (OR)
    assert_eq!(
        spec["paths"]["/x"]["get"]["security"],
        json!([{ "bearerAuth": ["read", "write"], "apiKey": [] }])
    );
}

fn api_users() -> Api<AppState> {
    Api::<AppState>::new()
        .get(
            "/greet/:path",
            Endpoint::new("greet")
                .path::<EchoPath>()
                .returns(200, JsonBody::new::<Greeting>()),
            |req, ctx| {
                async move {
                    Ok(Greeting {
                        message: format!("{}, {}", ctx.state.greeting, req.params.path),
                    })
                }
            },
        )
        .delete(
            "/greet",
            Endpoint::new("ungreet").returns(204, JsonBody::new::<()>()),
            |_, _| async move { Ok(()) },
        )
        .config(GroupConfig::new().tag("Greetings"))
}

#[tokio::test]
async fn builds_routes_with_api() {
    let api = Api::<AppState>::new().nest("/v1", api_users()).merge(Api::new().get(
        "/health",
        Endpoint::new("health").returns(200, TextBody::new()),
        |_, _| async move { Ok("ok".to_string()) },
    ));

    let keys: Vec<&str> = api.endpoints().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["get:/v1/greet/:path", "delete:/v1/greet", "get:/health"]);

    let spec = build_json(&BuilderConfig::new("t", "1"), api.endpoints()).unwrap();
    assert_eq!(spec["paths"]["/v1/greet/{path}"]["get"]["tags"], json!(["Greetings"]));
    assert!(spec["paths"]["/health"]["get"]["tags"].is_null());

    let app = create_api(Router::new(), api, Default::default())
        .unwrap()
        .with_state(AppState {
            greeting: "hello".into(),
        });

    let (status, body) = send_json(
        app.clone(),
        Request::get("/v1/greet/world").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!((status, body), (StatusCode::OK, json!({ "message": "hello, world" })));

    let (status, _, body) = send(app.clone(), Request::delete("/v1/greet").body(Body::empty()).unwrap()).await;
    assert_eq!((status, body), (StatusCode::NO_CONTENT, vec![]));

    let (status, _, body) = send(app, Request::get("/health").body(Body::empty()).unwrap()).await;
    assert_eq!((status, body), (StatusCode::OK, b"ok".to_vec()));
}
