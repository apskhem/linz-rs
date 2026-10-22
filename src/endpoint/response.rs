//! Response declarations and handler results.

use std::fmt;

use axum::body::Body;
use http::header::{
    HeaderName,
    HeaderValue,
};
use http::{
    HeaderMap,
    StatusCode,
};
use serde::Serialize;
use serde_json::Value;

use super::{
    BodyDescriptor,
    SenderBody,
};
use crate::internal::files::{
    self,
    FileStore,
};

/// A declared response of an endpoint.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ResponseSpec {
    /// A response with a validated body.
    Body(BodyDescriptor),
    /// A JSON:API error response (`true` shorthand).
    Declared,
    /// A JSON:API error response with a description.
    Described(String),
}

/// Values accepted by [`Endpoint::response`](super::Endpoint::response) for an endpoint whose response type is `R`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot declare a response of an endpoint returning `{R}`",
    note = "declare the main body response with `.returns(status, body)` first; further body responses must have the same type",
    note = "use an enum or `serde_json::Value` to return different shapes",
    note = "`true` or a description declares a JSON:API error response"
)]
pub trait IntoResponseSpec<R> {
    /// Converts into a response declaration.
    fn into_response_spec(self) -> ResponseSpec;
}

impl<R> IntoResponseSpec<R> for SenderBody<R> {
    fn into_response_spec(self) -> ResponseSpec {
        ResponseSpec::Body(self.descriptor)
    }
}

impl<R> IntoResponseSpec<R> for bool {
    fn into_response_spec(self) -> ResponseSpec {
        ResponseSpec::Declared
    }
}

impl<R> IntoResponseSpec<R> for &str {
    fn into_response_spec(self) -> ResponseSpec {
        ResponseSpec::Described(self.to_string())
    }
}

impl<R> IntoResponseSpec<R> for String {
    fn into_response_spec(self) -> ResponseSpec {
        ResponseSpec::Described(self)
    }
}

/// The response type of an [`Endpoint`](super::Endpoint) before `.returns(..)` is called.
#[derive(Debug, Clone, Copy)]
pub struct NoResponse;

/// A type an endpoint handler can respond with.
#[diagnostic::on_unimplemented(
    message = "the endpoint has no declared response type",
    label = "`.handler(..)` requires a response type",
    note = "call `.returns(status, body)` (or `.returns_default(body)`) before `.handler(..)`"
)]
pub trait ResponsePayload: Serialize + Send + 'static {}

impl<T: Serialize + Send + 'static> ResponsePayload for T {}

/// The payload of an [`HttpResponse`].
#[non_exhaustive]
pub enum ResponseBody<T> {
    /// No body.
    Empty,
    /// A body that is validated and serialized by the declared response.
    Data(T),
    /// A streamed body. It is neither validated nor serialized.
    Stream(Body),
}

impl<T: fmt::Debug> fmt::Debug for ResponseBody<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Empty"),
            Self::Data(data) => f.debug_tuple("Data").field(data).finish(),
            Self::Stream(_) => f.write_str("Stream(..)"),
        }
    }
}

/// The result of an endpoint handler.
///
/// When `status` is not set, `201 Created` is used for `post` endpoints and `200 OK` otherwise.
#[derive(Debug)]
pub struct HttpResponse<T> {
    /// The status. When `None`, see [`EndpointMeta::default_status`](super::EndpointMeta::default_status).
    pub status: Option<StatusCode>,
    /// Extra headers. They replace the headers of the same name set by linz-rs.
    pub headers: HeaderMap,
    /// The payload.
    pub body: ResponseBody<T>,
}

impl<T> HttpResponse<T> {
    /// A response with the default status.
    pub fn new(body: T) -> Self {
        Self {
            status: None,
            headers: HeaderMap::new(),
            body: ResponseBody::Data(body),
        }
    }

    /// A response with the given status.
    pub fn with_status(status: StatusCode, body: T) -> Self {
        Self::new(body).status(status)
    }

    /// A response without body.
    pub fn without_body(status: StatusCode, headers: Option<HeaderMap>) -> Self {
        Self {
            status: Some(status),
            headers: headers.unwrap_or_default(),
            body: ResponseBody::Empty,
        }
    }

    /// A streamed response. The body is neither validated nor serialized.
    pub fn stream(status: StatusCode, body: impl Into<Body>) -> Self {
        Self {
            status: Some(status),
            headers: HeaderMap::new(),
            body: ResponseBody::Stream(body.into()),
        }
    }

    /// Sets the status.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets a header.
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }
}

impl<T> From<T> for HttpResponse<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

/// The success value of a handler of an endpoint whose response type is `R`: either `R`
/// itself or an [`HttpResponse<R>`].
#[diagnostic::on_unimplemented(
    message = "the handler returns `{Self}`, but the endpoint's response type is `{R}`",
    label = "expected `{R}` or `HttpResponse<{R}>`",
    note = "the response type is set by `.returns(status, body)`"
)]
pub trait IntoHttpResponse<R> {
    /// Converts into a response.
    fn into_http_response(self) -> HttpResponse<R>;
}

impl<R> IntoHttpResponse<R> for R {
    fn into_http_response(self) -> HttpResponse<R> {
        HttpResponse::new(self)
    }
}

impl<R> IntoHttpResponse<R> for HttpResponse<R> {
    fn into_http_response(self) -> HttpResponse<R> {
        self
    }
}

type SerializeFn = Box<dyn FnOnce(bool) -> Result<(Value, FileStore), String> + Send>;

pub(crate) enum ErasedBody {
    Empty,
    Data(SerializeFn),
    Stream(Body),
}

pub(crate) struct ErasedResponse {
    pub status: Option<StatusCode>,
    pub headers: HeaderMap,
    pub body: ErasedBody,
}

impl<R: Serialize + Send + 'static> HttpResponse<R> {
    pub(crate) fn erase(self) -> ErasedResponse {
        let body = match self.body {
            ResponseBody::Empty => ErasedBody::Empty,
            ResponseBody::Stream(stream) => ErasedBody::Stream(stream),
            ResponseBody::Data(data) => {
                ErasedBody::Data(Box::new(move |with_files| {
                    if with_files {
                        let (value, files) = files::capture(|| serde_json::to_value(&data));
                        value.map(|v| (v, files)).map_err(|e| e.to_string())
                    } else {
                        serde_json::to_value(&data)
                            .map(|v| (v, FileStore::default()))
                            .map_err(|e| e.to_string())
                    }
                }))
            }
        };

        ErasedResponse {
            status: self.status,
            headers: self.headers,
            body,
        }
    }
}
