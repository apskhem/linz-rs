//! The validated request and the context passed to handlers, and how request parts are decoded.

use std::any::TypeId;

use http::{
    Extensions,
    StatusCode,
};
use serde_json::Value;

use super::DataTransferObject;
use crate::err::{
    ApiError,
    ValidationFieldError,
    ValidationIssue,
};
use crate::internal::files::{
    self,
    FileStore,
};
use crate::internal::schema;
use crate::security::AppliedSecurity;

/// A validated incoming request passed to the endpoint handler.
#[derive(Debug, Clone)]
pub struct Request<Q = (), H = (), P = (), C = (), B = ()> {
    /// The validated query string.
    pub queries: Q,
    /// The validated headers.
    pub headers: H,
    /// The validated path parameters.
    pub params: P,
    /// The validated cookies.
    pub cookies: C,
    /// The validated body.
    pub body: B,
}

/// The context of a request passed to the endpoint handler.
#[derive(Debug)]
pub struct Context<S = ()> {
    /// The state of the router, see [`axum::Router::with_state`].
    pub state: S,
    /// The applied securities of the endpoint.
    pub security: Vec<AppliedSecurity>,
    /// Objects attached by the security handlers.
    pub extensions: Extensions,
    /// The raw request head.
    pub request: http::request::Parts,
}

impl<S> Context<S> {
    /// Returns an object attached by a security handler, or a `500` error when it is missing,
    /// e.g. `let user = ctx.extension::<CurrentUser>()?;`.
    #[allow(clippy::result_large_err)]
    pub fn extension<T: Send + Sync + 'static>(&self) -> Result<&T, ApiError> {
        self.extensions.get::<T>().ok_or_else(|| {
            ApiError::detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Missing request extension `{}`", std::any::type_name::<T>()),
            )
        })
    }
}

/// A part of an incoming request in its intermediate JSON representation.
pub(crate) struct RawPart {
    pub value: Value,
    pub files: FileStore,
    /// Whether string values may be coerced to the types of the schema.
    pub coerce: bool,
    /// Whether object keys match the schema's property names case-insensitively.
    pub case_insensitive: bool,
}

impl RawPart {
    pub fn textual(value: Value) -> Self {
        Self {
            value,
            files: FileStore::default(),
            coerce: true,
            case_insensitive: false,
        }
    }

    pub fn case_insensitive(mut self) -> Self {
        self.case_insensitive = true;
        self
    }
}

pub(crate) struct RawInput {
    pub body: RawPart,
    pub queries: RawPart,
    pub params: RawPart,
    pub headers: RawPart,
    pub cookies: RawPart,
}

/// Renames the keys of an object to the property names of the schema they match
/// case-insensitively, e.g. `x-api-key` to `X-Api-Key`.
fn rename_keys_like_schema(value: &mut Value, root: &Value) {
    let (Value::Object(map), Some(shape)) = (value, schema::object_shape(root)) else {
        return;
    };

    for property in shape.properties {
        if map.contains_key(&property) {
            continue;
        }
        let key = map.keys().find(|k| k.eq_ignore_ascii_case(&property)).cloned();
        if let Some(item) = key.and_then(|key| map.shift_remove(&key)) {
            map.insert(property, item);
        }
    }
}

pub(crate) fn decode_part<T: DataTransferObject>(
    location: &'static str,
    part: RawPart,
    issues: &mut Vec<ValidationIssue>,
) -> Option<T> {
    // the part is not declared
    if TypeId::of::<T>() == TypeId::of::<()>() {
        return serde_json::from_value(Value::Null).ok();
    }

    let RawPart {
        mut value,
        files,
        coerce,
        case_insensitive,
    } = part;
    let compiled = schema::compiled::<T>();

    if case_insensitive {
        rename_keys_like_schema(&mut value, &compiled.root);
    }

    if coerce {
        schema::coerce(&mut value, &compiled.root);
    }

    let errors: Vec<ValidationFieldError> = compiled.validator.iter_errors(&value).map(Into::into).collect();
    if !errors.is_empty() {
        issues.push(ValidationIssue {
            location,
            result: serde_json::to_value(errors).unwrap_or_default(),
        });
        return None;
    }

    match files::with_inbox(&files, || serde_json::from_value::<T>(value)) {
        Ok(value) => Some(value),
        Err(err) => {
            issues.push(ValidationIssue {
                location,
                result: Value::String(err.to_string()),
            });
            None
        }
    }
}
