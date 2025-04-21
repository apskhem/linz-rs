use core::fmt;

use http::StatusCode;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ValidationFieldError {
    #[serde(skip_serializing)]
    pub kind: jsonschema::error::ValidationErrorKind,
    pub path: Vec<String>,
    pub message: String,
}

impl From<jsonschema::ValidationError<'_>> for ValidationFieldError {
    fn from(value: jsonschema::ValidationError<'_>) -> Self {
        let message = value.to_string();

        Self {
            kind: value.kind,
            path: value
                .instance_path
                .into_iter()
                .map(|seg| seg.to_string())
                .collect(),
            message,
        }
    }
}

#[derive(Debug, Serialize)]
pub enum PayloadError {
    InvalidData,
    FailedValidating(Vec<ValidationFieldError>),
    FailedDeserializing,
    FailedSerializing,
}

#[derive(Debug, Default, Serialize)]
pub struct RequestValidationError {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queries: Option<PayloadError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<PayloadError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<PayloadError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cookies: Option<PayloadError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<PayloadError>,
}

impl RequestValidationError {
    pub fn is_err(&self) -> bool {
        self.queries.is_some()
            || self.headers.is_some()
            || self.params.is_some()
            || self.cookies.is_some()
            || self.body.is_some()
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum SecurityError {
    MissingAuthorizationHeader,
    InvalidAuthorizationHeaderValue,
    MissingTokenFromScheme,
    InvalidScheme,
    MissingApiKeyParameter,
    InvalidUtf8HeaderValue,
    WithStatus((StatusCode, String)),
    Custom(String),
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAuthorizationHeader => f.write_str("Missing authorizaiton header"),
            Self::InvalidAuthorizationHeaderValue => f.write_str("Invalid authorizaiton header value"),
            Self::MissingTokenFromScheme => f.write_str("Missing authorizaiton token"),
            Self::InvalidScheme => f.write_str("Invalid authorizaiton scheme"),
            Self::MissingApiKeyParameter => f.write_str("Missing API key parameter"),
            Self::InvalidUtf8HeaderValue => f.write_str("Invalid UTF-8 header value"),
            _ => f.write_str("Failed authentication"),
        }
    }
}
