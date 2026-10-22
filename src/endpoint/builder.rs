//! The [`Endpoint`] builder and the type-erased [`LinzEndpoint`].

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

use oas3::spec;

use super::request::decode_part;
use super::{
    BodyDescriptor,
    BoxFuture,
    Context,
    DataTransferObject,
    ErasedResponse,
    IntoHttpResponse,
    IntoResponseSpec,
    NoResponse,
    RawInput,
    Request,
    ResponsePayload,
    ResponseSpec,
    SenderBody,
    TypeSchema,
};
use crate::err::{
    ApiError,
    ValidationIssue,
};
use crate::security::AppliedSecurity;

/// Schemas of the parameters of an endpoint.
#[derive(Debug, Clone, Default)]
pub struct ParameterSchemas {
    /// The schema of the query DTO.
    pub query: Option<TypeSchema>,
    /// The schema of the header DTO.
    pub header: Option<TypeSchema>,
    /// The schema of the path DTO.
    pub path: Option<TypeSchema>,
    /// The schema of the cookie DTO.
    pub cookie: Option<TypeSchema>,
}

impl ParameterSchemas {
    /// Whether no parameter is declared.
    pub fn is_empty(&self) -> bool {
        self.query.is_none() && self.header.is_none() && self.path.is_none() && self.cookie.is_none()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&'static str, &TypeSchema)> {
        [
            ("query", &self.query),
            ("header", &self.header),
            ("path", &self.path),
            ("cookie", &self.cookie),
        ]
        .into_iter()
        .filter_map(|(location, schema)| schema.as_ref().map(|s| (location, s)))
    }
}

/// Type-erased metadata of an endpoint.
#[derive(Debug, Clone, Default)]
pub struct EndpointMeta {
    /// The tags in the docs.
    pub tags: Vec<spec::Tag>,
    /// The summary in the docs.
    pub summary: Option<String>,
    /// The description in the docs.
    pub description: Option<String>,
    /// The unique operation ID.
    pub operation_id: String,
    /// Hides the endpoint from showing on the spec, but can still access.
    pub hidden: Option<bool>,
    /// Whether the endpoint is deprecated in the docs.
    pub deprecated: Option<bool>,
    /// The authentication steps, run in order.
    pub security: Vec<AppliedSecurity>,
    /// The schemas of the parameters.
    pub parameters: ParameterSchemas,
    /// The request body.
    pub request_body: Option<BodyDescriptor>,
    /// The declared responses, by status.
    pub responses: BTreeMap<u16, ResponseSpec>,
    /// The response for statuses without their own declaration.
    pub default_response: Option<BodyDescriptor>,
    /// The status used when the handler does not set one, set by [`Endpoint::returns`].
    /// Without it, `201` is used for `post` endpoints and `200` otherwise.
    pub default_status: Option<u16>,
}

/// Converts a tag name or an OpenAPI tag object into a tag.
pub trait IntoTag {
    /// Converts into a tag.
    fn into_tag(self) -> spec::Tag;
}

impl IntoTag for spec::Tag {
    fn into_tag(self) -> spec::Tag {
        self
    }
}

impl IntoTag for &str {
    fn into_tag(self) -> spec::Tag {
        self.to_string().into_tag()
    }
}

impl IntoTag for String {
    fn into_tag(self) -> spec::Tag {
        spec::Tag {
            name: self,
            description: None,
            extensions: Default::default(),
        }
    }
}

pub(crate) enum HandlerError {
    Validation(Vec<ValidationIssue>),
    Api(ApiError),
}

type ErasedHandler<S> =
    Arc<dyn Fn(RawInput, Context<S>) -> BoxFuture<Result<ErasedResponse, HandlerError>> + Send + Sync>;

/// A type-erased endpoint, created by [`Endpoint::handler`]. `S` is the router state.
pub struct LinzEndpoint<S = ()> {
    pub(crate) meta: EndpointMeta,
    pub(crate) handler: ErasedHandler<S>,
}

impl<S> Clone for LinzEndpoint<S> {
    fn clone(&self) -> Self {
        Self {
            meta: self.meta.clone(),
            handler: self.handler.clone(),
        }
    }
}

impl<S> LinzEndpoint<S> {
    /// The metadata of the endpoint.
    pub fn meta(&self) -> &EndpointMeta {
        &self.meta
    }

    /// The metadata of the endpoint, e.g. to adjust it after building.
    pub fn meta_mut(&mut self) -> &mut EndpointMeta {
        &mut self.meta
    }

    /// The operation ID of the endpoint.
    pub fn operation_id(&self) -> &str {
        &self.meta.operation_id
    }
}

impl<S> fmt::Debug for LinzEndpoint<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinzEndpoint")
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

/// A list of `method:path` keys (e.g. `get:/users/:id`) and their endpoints.
pub type LinzEndpointGroup<S = ()> = Vec<(String, LinzEndpoint<S>)>;

/// A builder of an endpoint. The type parameters are the query (`Q`), header (`H`),
/// path (`P`), cookie (`C`) and body (`B`) types of the validated [`Request`], and the
/// response type (`R`) set by [`Endpoint::returns`].
pub struct Endpoint<Q = (), H = (), P = (), C = (), B = (), R = NoResponse> {
    meta: EndpointMeta,
    _marker: TypeMarker<Q, H, P, C, B, R>,
}

type TypeMarker<Q, H, P, C, B, R> = PhantomData<fn() -> (Q, H, P, C, B, R)>;

impl Endpoint {
    /// Starts declaring an endpoint with a unique operation ID.
    pub fn new(operation_id: impl Into<String>) -> Self {
        Self {
            meta: EndpointMeta {
                operation_id: operation_id.into(),
                ..Default::default()
            },
            _marker: PhantomData,
        }
    }
}

impl<Q, H, P, C, B, R> Endpoint<Q, H, P, C, B, R> {
    fn retype<Q2, H2, P2, C2, B2, R2>(self) -> Endpoint<Q2, H2, P2, C2, B2, R2> {
        Endpoint {
            meta: self.meta,
            _marker: PhantomData,
        }
    }

    /// Adds a tag, given by name (`"Users"`) or as an [`oas::Tag`](spec::Tag) object.
    pub fn tag(mut self, tag: impl IntoTag) -> Self {
        self.meta.tags.push(tag.into_tag());
        self
    }

    /// Adds tags, given by name or as [`oas::Tag`](spec::Tag) objects.
    pub fn tags(mut self, tags: impl IntoIterator<Item = impl IntoTag>) -> Self {
        self.meta.tags.extend(tags.into_iter().map(IntoTag::into_tag));
        self
    }

    /// Sets the summary in the docs. Defaults to the operation ID.
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.meta.summary = Some(summary.into());
        self
    }

    /// Sets the description in the docs.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.meta.description = Some(description.into());
        self
    }

    /// Hides the endpoint from showing on the spec, but can still access.
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.meta.hidden = Some(hidden);
        self
    }

    /// Marks the endpoint as deprecated in the docs.
    pub fn deprecated(mut self, deprecated: bool) -> Self {
        self.meta.deprecated = Some(deprecated);
        self
    }

    /// Adds an authentication step: `scheme.apply([..scopes])`, or `&scheme` for no scopes.
    pub fn security(mut self, security: impl Into<AppliedSecurity>) -> Self {
        self.meta.security.push(security.into());
        self
    }

    /// Validates the query string into `req.queries: T`.
    pub fn query<T: DataTransferObject>(mut self) -> Endpoint<T, H, P, C, B, R> {
        self.meta.parameters.query = Some(TypeSchema::of::<T>());
        self.retype()
    }

    /// Validates the headers into `req.headers: T`. Field names match header names case-insensitively.
    pub fn header<T: DataTransferObject>(mut self) -> Endpoint<Q, T, P, C, B, R> {
        self.meta.parameters.header = Some(TypeSchema::of::<T>());
        self.retype()
    }

    /// Validates the path parameters into `req.params: T`. Its fields must match the parameters of the path.
    pub fn path<T: DataTransferObject>(mut self) -> Endpoint<Q, H, T, C, B, R> {
        self.meta.parameters.path = Some(TypeSchema::of::<T>());
        self.retype()
    }

    /// Validates the cookies into `req.cookies: T`.
    pub fn cookie<T: DataTransferObject>(mut self) -> Endpoint<Q, H, P, T, B, R> {
        self.meta.parameters.cookie = Some(TypeSchema::of::<T>());
        self.retype()
    }

    /// Validates the request body into `req.body: T`.
    pub fn request_body<T: DataTransferObject>(mut self, body: SenderBody<T>) -> Endpoint<Q, H, P, C, T, R> {
        self.meta.request_body = Some(body.descriptor);
        self.retype()
    }

    /// Declares another response. Accepts a [`SenderBody`] of the response type `R`, or
    /// `true`/a description, which declare a JSON:API error response.
    pub fn response(mut self, status: u16, spec: impl IntoResponseSpec<R>) -> Self {
        self.meta.responses.insert(status, spec.into_response_spec());
        self
    }

    /// Declares the response used for statuses that have no declaration.
    pub fn default_response(mut self, body: SenderBody<R>) -> Self {
        self.meta.default_response = Some(body.descriptor);
        self
    }
}

impl<Q, H, P, C, B> Endpoint<Q, H, P, C, B, NoResponse> {
    /// Declares the main response and sets the response type of the handler to `T`.
    /// Further body responses declared with [`Endpoint::response`] must also be of type `T`.
    /// `status` is used when the handler does not set one.
    ///
    /// ```
    /// use linz_rs::*;
    ///
    /// #[derive(serde::Serialize, schemars::JsonSchema)]
    /// struct User {
    ///     id: u32,
    /// }
    ///
    /// let endpoint: LinzEndpoint = Endpoint::new("getUser")
    ///     .returns(200, JsonBody::new::<User>())
    ///     .response(404, true)
    ///     .handler(|_, _| async move { Ok(User { id: 1 }) });
    /// ```
    ///
    /// Returning another type does not compile:
    ///
    /// ```compile_fail
    /// # use linz_rs::*;
    /// # #[derive(serde::Serialize, schemars::JsonSchema)]
    /// # struct User { id: u32 }
    /// let endpoint: LinzEndpoint = Endpoint::new("getUser")
    ///     .returns(200, JsonBody::new::<User>())
    ///     .handler(|_, _| async move { Ok("not a user".to_string()) });
    /// ```
    ///
    /// Neither does declaring a body response of another type:
    ///
    /// ```compile_fail
    /// # use linz_rs::*;
    /// # #[derive(serde::Serialize, schemars::JsonSchema)]
    /// # struct User { id: u32 }
    /// let endpoint: LinzEndpoint = Endpoint::new("getUser")
    ///     .returns(200, JsonBody::new::<User>())
    ///     .response(202, TextBody::new())
    ///     .handler(|_, _| async move { Ok(User { id: 1 }) });
    /// ```
    ///
    /// Nor finishing an endpoint without a response type:
    ///
    /// ```compile_fail
    /// # use linz_rs::*;
    /// let endpoint: LinzEndpoint = Endpoint::new("ping").handler(|_, _| async move { Ok(()) });
    /// ```
    pub fn returns<T>(mut self, status: u16, body: SenderBody<T>) -> Endpoint<Q, H, P, C, B, T> {
        self.meta.responses.insert(status, ResponseSpec::Body(body.descriptor));
        self.meta.default_status = Some(status);
        self.retype()
    }

    /// Like [`Endpoint::returns`], but declares the default response.
    pub fn returns_default<T>(mut self, body: SenderBody<T>) -> Endpoint<Q, H, P, C, B, T> {
        self.meta.default_response = Some(body.descriptor);
        self.retype()
    }
}

impl<Q, H, P, C, B, R> Endpoint<Q, H, P, C, B, R>
where
    Q: DataTransferObject,
    H: DataTransferObject,
    P: DataTransferObject,
    C: DataTransferObject,
    B: DataTransferObject,
    R: ResponsePayload,
{
    /// Sets the handler and finishes the endpoint.
    ///
    /// The handler returns `Ok` with either the response type `R` or an [`HttpResponse<R>`](super::HttpResponse),
    /// or `Err` with an [`ApiError`]. `S` is the state of the router, available as
    /// [`Context::state`]. Handlers that read the state annotate the context type, like
    /// axum's `State<S>` extractor:
    ///
    /// ```
    /// use linz_rs::*;
    ///
    /// #[derive(Clone)]
    /// struct AppState {
    ///     name: String,
    /// }
    ///
    /// fn greet() -> LinzEndpoint<AppState> {
    ///     Endpoint::new("greet")
    ///         .returns(200, TextBody::new())
    ///         .handler(|_, ctx: Context<AppState>| async move { Ok(format!("hello from {}", ctx.state.name)) })
    /// }
    ///
    /// let app: axum::Router = create_api(axum::Router::new(), vec![("get:/greet".into(), greet())], Default::default())
    ///     .unwrap()
    ///     .with_state(AppState { name: "linz".into() });
    /// ```
    pub fn handler<S, O, F, Fut>(self, handler: F) -> LinzEndpoint<S>
    where
        S: Send + 'static,
        O: IntoHttpResponse<R>,
        F: Fn(Request<Q, H, P, C, B>, Context<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, ApiError>> + Send + 'static,
    {
        let handler = Arc::new(handler);

        let erased: ErasedHandler<S> = Arc::new(move |input: RawInput, ctx: Context<S>| {
            let mut issues = Vec::new();

            let body = decode_part::<B>("body", input.body, &mut issues);
            let queries = decode_part::<Q>("queries", input.queries, &mut issues);
            let params = decode_part::<P>("params", input.params, &mut issues);
            let headers = decode_part::<H>("headers", input.headers, &mut issues);
            let cookies = decode_part::<C>("cookies", input.cookies, &mut issues);

            let request = match (queries, headers, params, cookies, body) {
                (Some(queries), Some(headers), Some(params), Some(cookies), Some(body)) if issues.is_empty() => {
                    Request {
                        queries,
                        headers,
                        params,
                        cookies,
                        body,
                    }
                }
                _ => return Box::pin(async move { Err(HandlerError::Validation(issues)) }),
            };

            let handler = handler.clone();
            Box::pin(async move {
                handler(request, ctx)
                    .await
                    .map(|output| output.into_http_response().erase())
                    .map_err(HandlerError::Api)
            })
        });

        LinzEndpoint {
            meta: self.meta,
            handler: erased,
        }
    }
}
