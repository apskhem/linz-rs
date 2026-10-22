//! Registers endpoints on an axum router: [`create_api`] and its configuration.

use std::collections::{
    BTreeMap,
    HashSet,
};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{
    FromRequestParts,
    RawPathParams,
    State,
};
use axum::response::Response;
use axum::routing::{
    MethodFilter,
    MethodRouter,
};
use http::{
    Extensions,
    HeaderMap,
    HeaderValue,
    Method,
    StatusCode,
    header,
};
use http_body_util::BodyExt;
use serde_json::Value;
use tower_http::cors::{
    AllowHeaders,
    Any,
    CorsLayer,
};

use crate::endpoint::*;
use crate::err::{
    ApiError,
    BodyParserError,
    JsonApiErrorItem,
    JsonApiErrorResponse,
    LinzError,
    ValidationIssue,
};
use crate::internal::files::FileStore;
use crate::internal::{
    body,
    schema,
    utils,
};
use crate::json_builder::{
    BuilderConfig,
    build_json,
};

/// A logger with the levels of RFC 5424. Every method is a no-op by default.
pub trait Logger: Send + Sync {
    /// Level 0
    fn emergency(&self, _message: &str) {}
    /// Level 1
    fn alert(&self, _message: &str) {}
    /// Level 2
    fn critical(&self, _message: &str) {}
    /// Level 3
    fn error(&self, _message: &str) {}
    /// Level 4
    fn warning(&self, _message: &str) {}
    /// Level 5
    fn notice(&self, _message: &str) {}
    /// Level 6
    fn info(&self, _message: &str) {}
    /// Level 7
    fn debug(&self, _message: &str) {}
}

/// CORS configurations.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
#[non_exhaustive]
pub enum CorsConfig {
    /// Allows any origin with the methods `GET`, `HEAD`, `PUT`, `PATCH`, `POST` and `DELETE`
    /// and mirrors the requested headers.
    Permissive,
    /// Uses the given `tower_http` CORS layer.
    Custom(CorsLayer),
}

impl CorsConfig {
    fn into_layer(self) -> CorsLayer {
        match self {
            Self::Permissive => {
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([
                        Method::GET,
                        Method::HEAD,
                        Method::PUT,
                        Method::PATCH,
                        Method::POST,
                        Method::DELETE,
                    ])
                    .allow_headers(AllowHeaders::mirror_request())
            }
            Self::Custom(layer) => layer,
        }
    }
}

/// The documents viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DocsViewer {
    /// [Scalar](https://github.com/scalar/scalar).
    Scalar,
    /// [Swagger UI](https://github.com/swagger-api/swagger-ui).
    Swagger,
    /// [Redoc](https://github.com/Redocly/redoc).
    Redoc,
    /// [RapiDoc](https://github.com/rapi-doc/RapiDoc).
    Rapidoc,
    /// [Stoplight Elements](https://github.com/stoplightio/elements).
    SpotlightElements,
}

impl DocsViewer {
    fn template(&self) -> &'static str {
        match self {
            Self::Scalar => include_str!("templates/scalar.html"),
            Self::Swagger => include_str!("templates/swagger.html"),
            Self::Redoc => include_str!("templates/redoc.html"),
            Self::Rapidoc => include_str!("templates/rapidoc.html"),
            Self::SpotlightElements => include_str!("templates/spotlight-elements.html"),
        }
    }
}

/// Configurations related to OpenAPI documents.
#[derive(Debug, Clone)]
pub struct DocsConfig {
    /// The documents viewer.
    pub viewer: DocsViewer,
    /// The configuration of the OpenAPI document, which is built from the endpoints
    /// passed to [`create_api`].
    pub spec: BuilderConfig,
    /// The path for users to view the docs. The docs will render to the set `viewer`.
    pub docs_path: String,
    /// The path to get the OpenAPI spec file in JSON.
    pub spec_path: String,
    /// The theme configurations for the docs viewer.
    pub theme: Option<String>,
}

/// A handler for requests that match no route. Returning `None` responds with `404 Not Found`.
pub type FallbackHandler = Arc<dyn Fn(axum::extract::Request) -> BoxFuture<Option<Response>> + Send + Sync>;

/// Configurations of [`create_api`].
#[derive(Clone, Default)]
pub struct CreateApiConfig {
    /// CORS configurations.
    pub cors: Option<CorsConfig>,
    /// Configurations related to OpenAPI documents.
    pub docs: Option<DocsConfig>,
    /// Receives the registration and error messages.
    pub logger: Option<Arc<dyn Logger>>,
    /// Called for requests that match no route, before the JSON `404` response.
    pub fallback_handler: Option<FallbackHandler>,
}

struct Shared {
    logger: Option<Arc<dyn Logger>>,
}

impl Shared {
    fn info(&self, message: &str) {
        if let Some(logger) = &self.logger {
            logger.info(message);
        }
    }

    fn error(&self, message: &str) {
        if let Some(logger) = &self.logger {
            logger.error(message);
        }
    }
}

/// Registers the endpoints (and optionally the docs, CORS and fallback) to the router.
///
/// Keys of `endpoints` are `method:path`, e.g. `get:/users/:id`. Path parameters can be
/// written as `:name` or `{name}`. When [`CreateApiConfig::docs`] is set, the OpenAPI
/// document is built from `endpoints`.
///
/// `S` is the state of the router, available to handlers as [`Context::state`].
pub fn create_api<S>(
    app: Router<S>,
    endpoints: impl Into<LinzEndpointGroup<S>>,
    config: CreateApiConfig,
) -> Result<Router<S>, LinzError>
where
    S: Clone + Send + Sync + 'static,
{
    let endpoints = endpoints.into();
    let spec = config
        .docs
        .as_ref()
        .map(|docs| build_json(&docs.spec, &endpoints))
        .transpose()?;

    let shared = Arc::new(Shared {
        logger: config.logger.clone(),
    });

    shared.info(&format!("Registering {} endpoints...", endpoints.len()));

    let mut routes: BTreeMap<String, (MethodRouter<S>, HashSet<Method>)> = BTreeMap::new();
    let mut add_route = |path: &str, method: Method, method_router: MethodRouter<S>| -> Result<(), LinzError> {
        let (axum_path, _) = utils::convert_path_params(path);
        let (existing, methods) = routes.remove(&axum_path).unwrap_or_default();

        if !methods.is_empty() && methods.contains(&method) {
            return Err(LinzError::DuplicateRoute {
                method: method.to_string(),
                path: path.to_string(),
            });
        }

        let mut methods = methods;
        methods.insert(method);
        routes.insert(axum_path, (existing.merge(method_router), methods));
        Ok(())
    };

    let mut registered_op_ids = HashSet::new();
    for (method_path, endpoint) in endpoints {
        let (method, path) = utils::split_method_path(&method_path)?;

        check_path_parameters(&endpoint, &path)?;

        if !registered_op_ids.insert(endpoint.operation_id().to_string()) {
            return Err(LinzError::DuplicateOperationId {
                operation_id: endpoint.operation_id().to_string(),
                path,
            });
        }

        shared.info(&format!(
            "{} -> {} {path}",
            endpoint.operation_id(),
            method.as_str().to_uppercase()
        ));

        let endpoint = Arc::new(endpoint);
        let shared = shared.clone();
        let handler = move |State(state): State<S>, req: axum::extract::Request| {
            let endpoint = endpoint.clone();
            let shared = shared.clone();
            async move { handle(&endpoint, method, &shared, state, req).await }
        };

        add_route(
            &path,
            method.to_method(),
            MethodRouter::new().on(method.method_filter(), handler),
        )?;
    }

    // docs config
    if let (Some(docs), Some(spec)) = (&config.docs, &spec) {
        let spec_json = serde_json::to_string(spec).map_err(|e| LinzError::Spec { message: e.to_string() })?;
        let title = &docs.spec.info.title;
        let html = docs
            .viewer
            .template()
            .replace("{{title}}", &escape_html(title))
            .replace("{{specUrl}}", &docs.spec_path)
            .replace("{{theme}}", docs.theme.as_deref().unwrap_or_default());

        add_route(
            &docs.spec_path,
            Method::GET,
            MethodRouter::new().on(MethodFilter::GET, move || {
                let spec_json = spec_json.clone();
                async move { raw_response(StatusCode::OK, mime::APPLICATION_JSON.as_ref(), spec_json) }
            }),
        )?;
        add_route(
            &docs.docs_path,
            Method::GET,
            MethodRouter::new().on(MethodFilter::GET, move || {
                let html = html.clone();
                async move { raw_response(StatusCode::OK, mime::TEXT_HTML.as_ref(), html) }
            }),
        )?;
    }

    let mut app = app;
    for (path, (method_router, _)) in routes {
        app = app.route(
            &path,
            method_router
                .fallback(|req: axum::extract::Request| async move { not_found(req.method(), req.uri().path()) }),
        );
    }

    // fallback
    let fallback_handler = config.fallback_handler.clone();
    app = app.fallback(move |req: axum::extract::Request| {
        let fallback_handler = fallback_handler.clone();
        async move {
            let method = req.method().clone();
            let path = req.uri().path().to_string();

            if let Some(fallback_handler) = fallback_handler
                && let Some(response) = fallback_handler(req).await
            {
                return response;
            }

            not_found(&method, &path)
        }
    });

    if let Some(cors) = config.cors {
        app = app.layer(cors.into_layer());
    }

    Ok(app)
}

/// Checks that the path parameters of the key match the fields of the path DTO.
fn check_path_parameters<S>(endpoint: &LinzEndpoint<S>, path: &str) -> Result<(), LinzError> {
    let Some(path_schema) = &endpoint.meta().parameters.path else {
        return Ok(());
    };
    let Some(shape) = schema::object_shape(&(path_schema.compiled)().root) else {
        return Ok(());
    };
    let parameters = utils::path_param_names(path);

    if !shape.open
        && let Some(parameter) = parameters.iter().find(|p| !shape.properties.contains(p))
    {
        return Err(LinzError::UndeclaredPathParameter {
            operation_id: endpoint.operation_id().to_string(),
            path: path.to_string(),
            parameter: parameter.clone(),
            declared: shape.properties,
        });
    }

    if let Some(field) = shape.required.iter().find(|f| !parameters.contains(f)) {
        return Err(LinzError::MissingPathParameter {
            operation_id: endpoint.operation_id().to_string(),
            path: path.to_string(),
            field: field.clone(),
        });
    }

    Ok(())
}

enum Failure {
    Api(ApiError),
    BodyParser(BodyParserError),
    Validation(Vec<ValidationIssue>),
    InvalidOutput(String),
    Internal(String),
}

async fn handle<S>(
    endpoint: &LinzEndpoint<S>,
    method: HttpMethod,
    shared: &Shared,
    state: S,
    req: axum::extract::Request,
) -> Response {
    match process(endpoint, method, shared, state, req).await {
        Ok(response) => response,
        Err(failure) => failure_response(failure, shared),
    }
}

async fn process<S>(
    endpoint: &LinzEndpoint<S>,
    method: HttpMethod,
    shared: &Shared,
    state: S,
    req: axum::extract::Request,
) -> Result<Response, Failure> {
    let meta = endpoint.meta();
    let (mut parts, body) = req.into_parts();

    let raw_params = RawPathParams::from_request_parts(&mut parts, &())
        .await
        .map_err(|e| Failure::Internal(e.to_string()))?;
    let params = Value::Object(
        raw_params
            .iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect(),
    );

    let body = body
        .collect()
        .await
        .map_err(|e| Failure::Internal(e.to_string()))?
        .to_bytes();
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let body = body::parse_body(&body, content_type).map_err(Failure::BodyParser)?;

    // process auth (if has) sequentially
    let mut extensions = Extensions::new();
    for applied in &meta.security {
        extensions.extend(applied.authenticate(&parts).await.map_err(Failure::Api)?);
    }

    let queries = utils::query_to_json(parts.uri.query());
    let cookies = utils::cookies_to_json(
        parts
            .headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
    );
    let headers = utils::headers_to_json(&parts.headers);

    let input = RawInput {
        body: match body {
            Some(parsed) => {
                RawPart {
                    value: parsed.value,
                    files: parsed.files,
                    coerce: parsed.textual,
                    case_insensitive: false,
                }
            }
            None => {
                RawPart {
                    value: Value::Null,
                    files: FileStore::default(),
                    coerce: false,
                    case_insensitive: false,
                }
            }
        },
        queries: RawPart::textual(queries),
        params: RawPart::textual(params),
        headers: RawPart::textual(headers).case_insensitive(),
        cookies: RawPart::textual(cookies),
    };
    let ctx = Context {
        state,
        security: meta.security.clone(),
        extensions,
        request: parts,
    };

    // validate and process main handler
    let result = (endpoint.handler)(input, ctx).await.map_err(|err| {
        match err {
            HandlerError::Validation(issues) => Failure::Validation(issues),
            HandlerError::Api(err) => Failure::Api(err),
        }
    })?;

    let status = result.status.unwrap_or_else(|| {
        meta.default_status
            .and_then(|status| StatusCode::from_u16(status).ok())
            .unwrap_or(match method {
                HttpMethod::Post => StatusCode::CREATED,
                _ => StatusCode::OK,
            })
    });

    let descriptor = match meta.responses.get(&status.as_u16()) {
        Some(ResponseSpec::Body(descriptor)) => Some(descriptor),
        Some(_) => None,
        None => meta.default_response.as_ref(),
    };
    let Some(descriptor) = descriptor else {
        shared.error(&format!(
            "There is no corresponding validator defined in schema for status {}/default",
            status.as_u16()
        ));
        return Err(Failure::Internal("Internal server error".to_string()));
    };

    // response
    let (value, files) = match result.body {
        ErasedBody::Stream(stream) => return Ok(build_response(status, HeaderMap::new(), result.headers, stream)),
        ErasedBody::Empty => (Value::Null, FileStore::default()),
        ErasedBody::Data(serialize) => serialize(descriptor.kind.carries_files()).map_err(Failure::InvalidOutput)?,
    };

    if descriptor.is_void() {
        if !value.is_null() {
            return Err(Failure::InvalidOutput("Expected no body".to_string()));
        }
        return Ok(build_response(status, HeaderMap::new(), result.headers, Body::empty()));
    }

    let errors = descriptor.schema.validate(&value);
    if !errors.is_empty() {
        return Err(Failure::InvalidOutput(
            serde_json::to_string(&errors).unwrap_or_default(),
        ));
    }

    if value.is_null() && descriptor.schema.is_option {
        return Ok(build_response(status, HeaderMap::new(), result.headers, Body::empty()));
    }

    let encoded = body::encode_body(descriptor.kind, &value, &files).map_err(Failure::InvalidOutput)?;
    let content_type = encoded
        .content_type
        .unwrap_or_else(|| descriptor.kind.mime_type().to_string());

    let mut base_headers = HeaderMap::new();
    if let Ok(content_type) = HeaderValue::from_str(&content_type) {
        base_headers.insert(header::CONTENT_TYPE, content_type);
    }

    Ok(build_response(
        status,
        base_headers,
        result.headers,
        Body::from(encoded.bytes),
    ))
}

/// Builds a response where `headers` override `base_headers`.
fn build_response(status: StatusCode, mut base_headers: HeaderMap, headers: HeaderMap, body: Body) -> Response {
    for name in headers.keys() {
        base_headers.remove(name);
    }
    base_headers.extend(headers);

    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = base_headers;
    response
}

fn raw_response(status: StatusCode, content_type: &str, body: impl Into<Body>) -> Response {
    let mut headers = HeaderMap::new();
    if let Ok(content_type) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, content_type);
    }
    build_response(status, headers, HeaderMap::new(), body.into())
}

fn error_response(status: StatusCode, errors: Vec<JsonApiErrorItem>, headers: HeaderMap) -> Response {
    let body = serde_json::to_vec(&JsonApiErrorResponse { errors }).unwrap_or_default();
    let mut base_headers = HeaderMap::new();
    base_headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    build_response(status, base_headers, headers, Body::from(body))
}

fn failure_response(failure: Failure, shared: &Shared) -> Response {
    match failure {
        Failure::Api(err) => error_response(err.status, err.errors, *err.headers),
        Failure::BodyParser(err) => {
            error_response(
                err.status,
                vec![JsonApiErrorItem::detail(err.message)],
                HeaderMap::new(),
            )
        }
        Failure::Validation(issues) => {
            let errors = issues
                .into_iter()
                .map(|issue| {
                    JsonApiErrorItem {
                        meta: serde_json::to_value(issue).ok().and_then(|v| v.as_object().cloned()),
                        ..Default::default()
                    }
                })
                .collect();
            error_response(StatusCode::BAD_REQUEST, errors, HeaderMap::new())
        }
        Failure::InvalidOutput(message) => {
            shared.error("Invalid output format to the corresponding defined output schema");
            shared.error(&message);
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                vec![JsonApiErrorItem::detail("Internal server error")],
                HeaderMap::new(),
            )
        }
        Failure::Internal(message) => {
            shared.error(&message);
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                vec![JsonApiErrorItem::detail(message)],
                HeaderMap::new(),
            )
        }
    }
}

fn not_found(method: &Method, path: &str) -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        vec![JsonApiErrorItem::detail(format!("Cannot find {method} {path}"))],
        HeaderMap::new(),
    )
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
