//! Endpoint declarations: [`Endpoint`], request and response bodies, [`HttpResponse`] and [`Context`].

mod body;
mod builder;
mod dto;
mod file;
mod method;
mod request;
mod response;

use std::future::Future;
use std::pin::Pin;

pub use self::body::{
    BodyDescriptor,
    BodyKind,
    EncodingItem,
    FormDataBody,
    HtmlBody,
    JsonBody,
    OctetStreamBody,
    SenderBody,
    TextBody,
    UrlEncodedBody,
};
pub(crate) use self::builder::HandlerError;
pub use self::builder::{
    Endpoint,
    EndpointMeta,
    IntoTag,
    LinzEndpoint,
    LinzEndpointGroup,
    ParameterSchemas,
};
pub use self::dto::{
    DataTransferObject,
    TypeSchema,
};
pub use self::file::File;
pub use self::method::{
    HttpMethod,
    METHODS,
};
pub use self::request::{
    Context,
    Request,
};
pub(crate) use self::request::{
    RawInput,
    RawPart,
};
pub(crate) use self::response::{
    ErasedBody,
    ErasedResponse,
};
pub use self::response::{
    HttpResponse,
    IntoHttpResponse,
    IntoResponseSpec,
    NoResponse,
    ResponseBody,
    ResponsePayload,
    ResponseSpec,
};

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
