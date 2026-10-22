//! Binary payloads.

use std::borrow::Cow;

use bytes::Bytes;
use schemars::{
    JsonSchema,
    Schema,
    SchemaGenerator,
    json_schema,
};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
};

use crate::internal::files;

/// A binary payload, either a file of a `multipart/form-data` body or a whole
/// `application/octet-stream` body.
///
/// It can only be (de)serialized as part of [`FormDataBody`](super::FormDataBody) or [`OctetStreamBody`](super::OctetStreamBody) payloads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct File {
    /// The filename of a multipart part, if any.
    pub name: Option<String>,
    /// The `Content-Type` of the multipart part or body, if any.
    pub content_type: Option<String>,
    /// The content.
    pub data: Bytes,
}

impl File {
    /// Creates a file without name and content type.
    pub fn new(data: impl Into<Bytes>) -> Self {
        Self {
            data: data.into(),
            ..Default::default()
        }
    }

    /// Sets the filename.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the content type.
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }
}

impl Serialize for File {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match files::put_into_outbox(self) {
            Some(placeholder) => serializer.serialize_str(&placeholder),
            None => {
                Err(serde::ser::Error::custom(
                    "`File` can only be serialized in a multipart/form-data or application/octet-stream body",
                ))
            }
        }
    }
}

impl<'de> Deserialize<'de> for File {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let placeholder = String::deserialize(deserializer)?;

        files::take_from_inbox(&placeholder).ok_or_else(|| serde::de::Error::custom("expected a file"))
    }
}

impl JsonSchema for File {
    fn schema_name() -> Cow<'static, str> {
        "File".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "format": "binary",
            "contentMediaType": "application/octet-stream",
        })
    }
}
