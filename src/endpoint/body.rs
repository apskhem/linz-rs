//! Request and response bodies: [`SenderBody`] and its constructors.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use oas3::spec;
use schemars::JsonSchema;

use super::{
    File,
    TypeSchema,
};

/// The kind of payload of a request or response body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BodyKind {
    /// `application/json`
    Json,
    /// `multipart/form-data`
    FormData,
    /// `application/x-www-form-urlencoded`
    UrlEncoded,
    /// `application/octet-stream`
    OctetStream,
    /// `text/plain`
    Text,
    /// `text/html`
    Html,
}

impl BodyKind {
    /// The MIME type, e.g. `application/json`.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::FormData => "multipart/form-data",
            Self::UrlEncoded => "application/x-www-form-urlencoded",
            Self::OctetStream => "application/octet-stream",
            Self::Text => "text/plain",
            Self::Html => "text/html",
        }
    }

    /// Whether serialized values may contain [`File`]s.
    pub(crate) fn carries_files(&self) -> bool {
        matches!(self, Self::FormData | Self::OctetStream)
    }
}

/// Encoding of a single property of a `multipart/form-data` or
/// `application/x-www-form-urlencoded` body.
#[derive(Debug, Clone, Default)]
pub struct EncodingItem {
    /// The allowed content types of the property.
    pub content_type: Vec<String>,
    /// The headers of a multipart part.
    pub headers: Option<TypeSchema>,
    /// How the property is serialized, see the OpenAPI `style` values.
    pub style: Option<String>,
    /// Whether arrays and objects generate separate parameters.
    pub explode: Option<bool>,
    /// Whether reserved characters are allowed without percent-encoding.
    pub allow_reserved: Option<bool>,
}

impl EncodingItem {
    /// Creates an empty encoding.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the allowed content types of the property.
    pub fn content_type(mut self, content_types: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.content_type = content_types.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the headers of a multipart part, described by the fields of `H`.
    pub fn headers<H: JsonSchema + 'static>(mut self) -> Self {
        self.headers = Some(TypeSchema::of::<H>());
        self
    }

    /// Sets the serialization style.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Sets whether arrays and objects generate separate parameters.
    pub fn explode(mut self, explode: bool) -> Self {
        self.explode = Some(explode);
        self
    }

    /// Sets whether reserved characters are allowed without percent-encoding.
    pub fn allow_reserved(mut self, allow_reserved: bool) -> Self {
        self.allow_reserved = Some(allow_reserved);
        self
    }
}

/// The type-erased description of a request or response body.
#[derive(Debug, Clone)]
pub struct BodyDescriptor {
    /// The content type of the body.
    pub kind: BodyKind,
    /// The schema of the payload.
    pub schema: TypeSchema,
    /// The description in the docs.
    pub description: Option<String>,
    /// Headers of a response (documentation only).
    pub headers: Option<TypeSchema>,
    /// Documented examples, by name.
    pub examples: BTreeMap<String, spec::Example>,
    /// The encoding of properties of form bodies, by property name.
    pub encoding: BTreeMap<String, EncodingItem>,
}

impl BodyDescriptor {
    /// Whether the body has no content, i.e. its type is `()`.
    pub fn is_void(&self) -> bool {
        self.schema.is_unit
    }
}

/// A typed request or response body. Create it with [`JsonBody`], [`FormDataBody`],
/// [`UrlEncodedBody`], [`OctetStreamBody`], [`TextBody`] or [`HtmlBody`].
pub struct SenderBody<T> {
    pub(crate) descriptor: BodyDescriptor,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for SenderBody<T> {
    fn clone(&self) -> Self {
        Self {
            descriptor: self.descriptor.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T> fmt::Debug for SenderBody<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.descriptor.fmt(f)
    }
}

impl<T: JsonSchema + 'static> SenderBody<T> {
    /// Creates a body of the given kind. Prefer the constructors such as [`JsonBody::new`].
    pub fn new(kind: BodyKind) -> Self {
        Self {
            descriptor: BodyDescriptor {
                kind,
                schema: TypeSchema::of::<T>(),
                description: None,
                headers: None,
                examples: BTreeMap::new(),
                encoding: BTreeMap::new(),
            },
            _marker: PhantomData,
        }
    }
}

impl<T> SenderBody<T> {
    /// Sets the description in the docs.
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.descriptor.description = Some(description.into());
        self
    }

    /// Declares the headers of a response.
    pub fn require_headers<H: JsonSchema + 'static>(mut self) -> Self {
        self.descriptor.headers = Some(TypeSchema::of::<H>());
        self
    }

    /// Sets documented examples, by name.
    pub fn set_examples(mut self, examples: impl IntoIterator<Item = (impl Into<String>, spec::Example)>) -> Self {
        self.descriptor.examples = examples.into_iter().map(|(k, v)| (k.into(), v)).collect();
        self
    }

    /// Sets the encoding of a property (`multipart/form-data` and
    /// `application/x-www-form-urlencoded` only).
    pub fn encoding(mut self, property: impl Into<String>, item: EncodingItem) -> Self {
        self.descriptor.encoding.insert(property.into(), item);
        self
    }

    /// The type-erased description of the body.
    pub fn descriptor(&self) -> &BodyDescriptor {
        &self.descriptor
    }
}

/// `application/json` body.
pub struct JsonBody;

impl JsonBody {
    /// `application/json`
    pub const MIME_TYPE: &'static str = "application/json";

    /// A JSON body of type `T`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T: JsonSchema + 'static>() -> SenderBody<T> {
        SenderBody::new(BodyKind::Json)
    }
}

/// `multipart/form-data` body. Use [`File`] for file fields.
pub struct FormDataBody;

impl FormDataBody {
    /// `multipart/form-data`
    pub const MIME_TYPE: &'static str = "multipart/form-data";

    /// A multipart body of type `T`. Use [`File`] for file fields.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T: JsonSchema + 'static>() -> SenderBody<T> {
        SenderBody::new(BodyKind::FormData)
    }
}

/// `application/x-www-form-urlencoded` body.
pub struct UrlEncodedBody;

impl UrlEncodedBody {
    /// `application/x-www-form-urlencoded`
    pub const MIME_TYPE: &'static str = "application/x-www-form-urlencoded";

    /// A url-encoded body of type `T`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T: JsonSchema + 'static>() -> SenderBody<T> {
        SenderBody::new(BodyKind::UrlEncoded)
    }
}

/// `application/octet-stream` body, represented by [`File`].
pub struct OctetStreamBody;

impl OctetStreamBody {
    /// `application/octet-stream`
    pub const MIME_TYPE: &'static str = "application/octet-stream";

    /// A binary body, represented by [`File`].
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> SenderBody<File> {
        SenderBody::new(BodyKind::OctetStream)
    }
}

/// `text/plain` body.
pub struct TextBody;

impl TextBody {
    /// `text/plain`
    pub const MIME_TYPE: &'static str = "text/plain";

    /// A plain-text body.
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> SenderBody<String> {
        SenderBody::new(BodyKind::Text)
    }
}

/// `text/html` body.
pub struct HtmlBody;

impl HtmlBody {
    /// `text/html`
    pub const MIME_TYPE: &'static str = "text/html";

    /// An HTML body.
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> SenderBody<String> {
        SenderBody::new(BodyKind::Html)
    }
}
