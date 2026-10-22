use std::fmt;
use std::future::Future;

use crate::core::{
    Context,
    DataTransferObject,
    Endpoint,
    HttpMethod,
    IntoHttpResponse,
    LinzEndpoint,
    LinzEndpointGroup,
    Request,
    ResponsePayload,
};
use crate::err::ApiError;
use crate::grouping::{
    GroupConfig,
    apply_group_config,
};
use crate::internal::utils;

/// A builder of an endpoint group. `S` is the router state, available to handlers as
/// [`Context::state`]. Name the state type once on the builder (`Api::<AppState>::new()`);
/// the handlers then need no annotations.
///
/// ```
/// use linz_rs::*;
///
/// #[derive(serde::Deserialize, schemars::JsonSchema)]
/// struct UserPath {
///     id: u32,
/// }
///
/// #[derive(serde::Serialize, schemars::JsonSchema)]
/// struct User {
///     id: u32,
/// }
///
/// #[derive(Clone)]
/// struct AppState {
///     prefix: String,
/// }
///
/// fn users() -> Api<AppState> {
///     Api::<AppState>::new()
///         .get(
///             "/users/:id",
///             Endpoint::new("getUser").path::<UserPath>().returns(200, JsonBody::new::<User>()),
///             |req, ctx| async move {
///                 let _ = &ctx.state.prefix;
///                 Ok(User { id: req.params.id })
///             },
///         )
///         .config(GroupConfig::new().tag("Users"))
/// }
///
/// let api = Api::new().nest("/v1", users());
/// let app: axum::Router = create_api(axum::Router::new(), api, Default::default())
///     .unwrap()
///     .with_state(AppState { prefix: "u".into() });
/// ```
pub struct Api<S = ()> {
    endpoints: LinzEndpointGroup<S>,
}

impl<S> Default for Api<S> {
    fn default() -> Self {
        Self { endpoints: Vec::new() }
    }
}

impl<S> Clone for Api<S> {
    fn clone(&self) -> Self {
        Self {
            endpoints: self.endpoints.clone(),
        }
    }
}

impl<S> fmt::Debug for Api<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Api").field("endpoints", &self.endpoints).finish()
    }
}

macro_rules! method_fns {
    ($($(#[$doc:meta])* $name:ident => $method:ident),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $name<Q, H, P, C, B, R, O, F, Fut>(self, path: &str, endpoint: Endpoint<Q, H, P, C, B, R>, handler: F) -> Self
            where
                Q: DataTransferObject,
                H: DataTransferObject,
                P: DataTransferObject,
                C: DataTransferObject,
                B: DataTransferObject,
                R: ResponsePayload,
                O: IntoHttpResponse<R>,
                F: Fn(Request<Q, H, P, C, B>, Context<S>) -> Fut + Send + Sync + 'static,
                Fut: Future<Output = Result<O, ApiError>> + Send + 'static,
            {
                self.route(HttpMethod::$method, path, endpoint, handler)
            }
        )*
    };
}

impl<S: Send + 'static> Api<S> {
    method_fns! {
        /// Adds a `GET` endpoint. Path parameters are written as `:name` or `{name}`.
        get => Get,
        /// Adds a `POST` endpoint. Path parameters are written as `:name` or `{name}`.
        post => Post,
        /// Adds a `PUT` endpoint. Path parameters are written as `:name` or `{name}`.
        put => Put,
        /// Adds a `PATCH` endpoint. Path parameters are written as `:name` or `{name}`.
        patch => Patch,
        /// Adds a `DELETE` endpoint. Path parameters are written as `:name` or `{name}`.
        delete => Delete,
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an endpoint with the given method and its handler.
    pub fn route<Q, H, P, C, B, R, O, F, Fut>(
        self,
        method: HttpMethod,
        path: &str,
        endpoint: Endpoint<Q, H, P, C, B, R>,
        handler: F,
    ) -> Self
    where
        Q: DataTransferObject,
        H: DataTransferObject,
        P: DataTransferObject,
        C: DataTransferObject,
        B: DataTransferObject,
        R: ResponsePayload,
        O: IntoHttpResponse<R>,
        F: Fn(Request<Q, H, P, C, B>, Context<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, ApiError>> + Send + 'static,
    {
        self.endpoint(method, path, endpoint.handler(handler))
    }

    /// Adds a finished endpoint, e.g. one built with [`Endpoint::handler`].
    pub fn endpoint(mut self, method: HttpMethod, path: &str, endpoint: LinzEndpoint<S>) -> Self {
        self.endpoints.push((format!("{method}:{path}"), endpoint));
        self
    }

    /// Adds every endpoint of `other` (an [`Api`] or a [`LinzEndpointGroup`]) with `prefix`
    /// prepended to its path. Consecutive slashes are collapsed.
    pub fn nest(mut self, prefix: &str, other: impl Into<LinzEndpointGroup<S>>) -> Self {
        for (key, endpoint) in other.into() {
            // invalid keys are kept as they are and reported by `create_api`
            let key = match utils::split_method_path(&key) {
                Ok((method, path)) => format!("{method}:{}", utils::clean_path(&format!("{prefix}{path}"))),
                Err(_) => key,
            };
            self.endpoints.push((key, endpoint));
        }
        self
    }

    /// Adds every endpoint of `other` (an [`Api`] or a [`LinzEndpointGroup`]).
    pub fn merge(self, other: impl Into<LinzEndpointGroup<S>>) -> Self {
        self.nest("", other)
    }

    /// Applies a common configuration (tags, security, hidden, deprecated) to every endpoint
    /// added so far.
    pub fn config(self, config: GroupConfig) -> Self {
        Self {
            endpoints: apply_group_config(self.endpoints, config),
        }
    }

    /// The `method:path` keys and endpoints added so far.
    pub fn endpoints(&self) -> &[(String, LinzEndpoint<S>)] {
        &self.endpoints
    }

    pub fn into_endpoints(self) -> LinzEndpointGroup<S> {
        self.endpoints
    }
}

impl<S> From<Api<S>> for LinzEndpointGroup<S> {
    fn from(api: Api<S>) -> Self {
        api.endpoints
    }
}

impl<S> From<LinzEndpointGroup<S>> for Api<S> {
    fn from(endpoints: LinzEndpointGroup<S>) -> Self {
        Self { endpoints }
    }
}
