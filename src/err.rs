//! Errors: [`ApiError`] for responses, [`LinzError`] for registration, and validation details.

use std::fmt;

use http::{
    HeaderMap,
    StatusCode,
};
use schemars::JsonSchema;
use serde::{
    Deserialize,
    Serialize,
};

/// A single error object conforming to JSON:API specification.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JsonApiErrorItem {
    /// A unique identifier for this particular occurrence of the problem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Links that lead to further details about the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<JsonApiErrorLinks>,
    /// HTTP status code applicable to this error, represented as a string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// An application-specific error code identifying the type of error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// A short, human-readable summary of the problem that should not change from occurrence to occurrence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A human-readable explanation specific to this occurrence of the problem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// An object containing references to the source of the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<JsonApiErrorSource>,
    /// A meta object containing non-standard, implementation-specific details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Map<String, serde_json::Value>>,
}

impl JsonApiErrorItem {
    /// Creates an error item containing only `detail`.
    pub fn detail(detail: impl Into<String>) -> Self {
        Self {
            detail: Some(detail.into()),
            ..Default::default()
        }
    }
}

/// Links that lead to further details about the error.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JsonApiErrorLinks {
    /// A link that leads to further details about this particular error.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(url)]
    pub about: Option<String>,
}

/// An object containing references to the source of the error.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JsonApiErrorSource {
    /// JSON Pointer to the request entity where the error originated (e.g., /data/attributes/email).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// Query parameter name that caused the error (e.g., 'sort').
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    /// A string indicating the name of a single request header which caused the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
}

/// JSON:API error response object.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JsonApiErrorResponse {
    /// Array of JSON:API error objects.
    pub errors: Vec<JsonApiErrorItem>,
}

/// An error that can be returned from endpoint and security handlers.
/// It is rendered as a JSON:API error response with the given status.
#[derive(Debug, Clone)]
pub struct ApiError {
    /// The status of the response.
    pub status: StatusCode,
    /// The JSON:API error objects of the response.
    pub errors: Vec<JsonApiErrorItem>,
    /// Extra headers of the response.
    pub headers: HeaderMap,
}

impl ApiError {
    /// Creates an error with a single error object.
    pub fn new(status: StatusCode, error: JsonApiErrorItem) -> Self {
        Self::many(status, vec![error])
    }

    /// Creates an error with several error objects.
    pub fn many(status: StatusCode, errors: Vec<JsonApiErrorItem>) -> Self {
        Self {
            status,
            errors,
            headers: HeaderMap::new(),
        }
    }

    /// Shorthand for an error containing a single item with only `detail`.
    pub fn detail(status: StatusCode, detail: impl Into<String>) -> Self {
        Self::new(status, JsonApiErrorItem::detail(detail))
    }

    /// Sets extra headers of the response.
    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "API error {}", self.status)
    }
}

impl std::error::Error for ApiError {}

/// A violation of a JSON schema by a request part.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationFieldError {
    /// The path to the invalid value, e.g. `["items", "0", "name"]`.
    pub path: Vec<String>,
    /// A human-readable description of the violation.
    pub message: String,
}

impl From<jsonschema::ValidationError<'_>> for ValidationFieldError {
    fn from(value: jsonschema::ValidationError<'_>) -> Self {
        let message = value.to_string();

        Self {
            path: value.instance_path().iter().map(|seg| seg.to_string()).collect(),
            message,
        }
    }
}

/// A validation issue of one part of an incoming request.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    /// One of `body`, `queries`, `params`, `headers` or `cookies`.
    #[serde(rename = "in")]
    pub location: &'static str,
    /// Either a list of [`ValidationFieldError`] or a message.
    pub result: serde_json::Value,
}

#[derive(Debug, Clone)]
pub(crate) struct BodyParserError {
    pub status: StatusCode,
    pub message: String,
}

impl BodyParserError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

/// A failure to extract the credentials of a [`crate::security::SecurityType`] from a request.
/// It responds `401` with its message as `detail`.
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
#[non_exhaustive]
pub enum SecurityError {
    /// The `Authorization` header is missing.
    MissingAuthorizationHeader,
    /// The `Authorization` header is not valid UTF-8 or has no scheme.
    InvalidAuthorizationHeaderValue,
    /// The `Authorization` header has a scheme but no credentials.
    MissingTokenFromScheme,
    /// The `Authorization` header uses another scheme.
    InvalidScheme,
    /// The header, query parameter or cookie of an API key is missing.
    MissingApiKeyParameter,
    /// The header of an API key is not valid UTF-8.
    InvalidUtf8HeaderValue,
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAuthorizationHeader => f.write_str("Missing authorization header"),
            Self::InvalidAuthorizationHeaderValue => f.write_str("Invalid authorization header value"),
            Self::MissingTokenFromScheme => f.write_str("Missing authorization token"),
            Self::InvalidScheme => f.write_str("Invalid authorization scheme"),
            Self::MissingApiKeyParameter => f.write_str("Missing API key parameter"),
            Self::InvalidUtf8HeaderValue => f.write_str("Invalid UTF-8 header value"),
        }
    }
}

impl std::error::Error for SecurityError {}

impl From<SecurityError> for ApiError {
    fn from(value: SecurityError) -> Self {
        ApiError::detail(StatusCode::UNAUTHORIZED, value.to_string())
    }
}

/// An error raised while registering endpoints or building the OpenAPI document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinzError {
    /// The key is not a `method:path` string.
    InvalidKey {
        /// The invalid key.
        key: String,
    },
    /// The method is not one of `get`, `post`, `put`, `patch` or `delete`.
    InvalidMethod {
        /// The invalid method.
        method: String,
        /// The path of the key.
        path: String,
    },
    /// The path does not start with `/`, or contains `?` or `#`.
    InvalidPath {
        /// The invalid path.
        path: String,
    },
    /// Two endpoints share an operation ID.
    DuplicateOperationId {
        /// The duplicated operation ID.
        operation_id: String,
        /// The path of the second endpoint.
        path: String,
    },
    /// Two routes share a method and path.
    DuplicateRoute {
        /// The method of the route.
        method: String,
        /// The path of the route.
        path: String,
    },
    /// Merging groups produced duplicated keys.
    DuplicateKeys {
        /// The duplicated `method:path` keys.
        keys: Vec<String>,
    },
    /// The path has a parameter that the path DTO does not declare.
    UndeclaredPathParameter {
        /// The operation ID of the endpoint.
        operation_id: String,
        /// The path of the endpoint.
        path: String,
        /// The undeclared parameter.
        parameter: String,
        /// The fields the path DTO declares.
        declared: Vec<String>,
    },
    /// The path DTO requires a field that the path does not provide.
    MissingPathParameter {
        /// The operation ID of the endpoint.
        operation_id: String,
        /// The path of the endpoint.
        path: String,
        /// The required field that is missing from the path.
        field: String,
    },
    /// The OpenAPI document could not be built.
    Spec {
        /// The underlying error.
        message: String,
    },
}

impl fmt::Display for LinzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey { key } => write!(f, "'{key}' is not a valid `method:path` key"),
            Self::InvalidMethod { method, path } => write!(f, "Invalid method \"{method}\" for path {path}"),
            Self::InvalidPath { path } => write!(f, "'{path}' is not a valid path"),
            Self::DuplicateOperationId { operation_id, path } => {
                write!(f, "Duplicate operation ID \"{operation_id}\" for path {path}")
            }
            Self::DuplicateRoute { method, path } => write!(f, "Duplicate route {method} {path}"),
            Self::DuplicateKeys { keys } => write!(f, "Duplicated keys occured: {}", keys.join(", ")),
            Self::UndeclaredPathParameter {
                operation_id,
                path,
                parameter,
                declared,
            } => {
                write!(
                    f,
                    "Path parameter \"{parameter}\" of {path} ({operation_id}) is not declared in its path type"
                )?;
                if declared.is_empty() {
                    write!(f, ", which declares no fields")
                } else {
                    write!(f, " (declared: {})", declared.join(", "))
                }
            }
            Self::MissingPathParameter {
                operation_id,
                path,
                field,
            } => {
                write!(
                    f,
                    "The path type of {path} ({operation_id}) requires \"{field}\", which is not a parameter of the path"
                )
            }
            Self::Spec { message } => write!(f, "Cannot build the OpenAPI document: {message}"),
        }
    }
}

impl std::error::Error for LinzError {}
