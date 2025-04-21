use core::fmt;

use http::StatusCode;

#[derive(Debug)]
pub struct ValidationFieldError {
    pub kind: jsonschema::error::ValidationErrorKind,
    pub path: String,
}

impl From<jsonschema::ValidationError<'_>> for ValidationFieldError {
    fn from(value: jsonschema::ValidationError<'_>) -> Self {
        Self {
            kind: value.kind,
            path: value.instance_path.to_string(),
        }
    }
}

#[derive(Debug)]
pub enum PayloadError {
    InvalidData(serde_json::Error),
    FailedValidating(Vec<ValidationFieldError>),
    FailedDeserializing(serde_json::Error),
    FailedSerializing(serde_json::Error),
}

#[derive(Debug, Default)]
pub struct RequestValidationError {
    pub queries: Option<PayloadError>,
    pub headers: Option<PayloadError>,
    pub params: Option<PayloadError>,
    pub cookies: Option<PayloadError>,
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

    pub fn into_response() {}
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
            _ => f.write_str("Failed authentication"),
        }
    }
}
