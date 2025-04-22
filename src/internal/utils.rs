use std::convert::identity;

use axum::body::Body;
use axum::response::Response;
use bytes::Bytes;
use http::{
    header,
    HeaderMap,
    HeaderValue,
    StatusCode,
};
use regex::Regex;
use serde_json::Value;

use crate::err::SecurityError;
use crate::security;

pub(crate) fn headers_to_json(headers: &HeaderMap<HeaderValue>) -> Value {
    Value::Object(
        headers
            .iter()
            .filter_map(|(key, value)| {
                value
                    .to_str()
                    .map(|value_str| (key.to_string(), Value::String(value_str.to_string())))
                    .ok()
            })
            .collect(),
    )
}

pub(crate) fn handle_response_error(status: StatusCode, msg: impl ToString) -> Response<Body> {
    let mut json_body = serde_json::Map::with_capacity(2);

    json_body.insert("code".into(), status.as_u16().into());
    json_body.insert("message".into(), msg.to_string().into());

    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime::APPLICATION_JSON.to_string())
        .body(Body::from(serde_json::Value::Object(json_body).to_string()))
        .unwrap()
}

pub(crate) fn handle_internal_error(msg: impl ToString) -> Response<Body> {
    handle_response_error(StatusCode::INTERNAL_SERVER_ERROR, msg)
}

pub(crate) fn extract_auth_header(
    req: &http::Request<Bytes>,
    required_scheme: &security::AuthenticationScheme,
) -> Result<String, SecurityError> {
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .ok_or_else(|| SecurityError::MissingAuthorizationHeader)?;

    let mut parts = auth
        .to_str()
        .map_err(|_| SecurityError::InvalidAuthorizationHeaderValue)?
        .trim()
        .splitn(2, char::is_whitespace);
    let decoded_scheme = parts
        .next()
        .ok_or_else(|| SecurityError::InvalidAuthorizationHeaderValue)?;
    let token = parts
        .next()
        .ok_or_else(|| SecurityError::InvalidAuthorizationHeaderValue)?;

    if decoded_scheme != required_scheme.to_string() {
        return Err(SecurityError::InvalidScheme);
    }

    Ok(String::from(token))
}

/// Parses a `Cookie` header string into key-value pairs according to RFC 6265.
///
/// # Arguments
///
/// * `cookie_header` - A string slice containing the raw `Cookie` header.
///
/// # Returns
///
/// A vector of `(String, String)` tuples representing cookie name-value pairs.
/// Pairs without a `=` are treated as having an empty value.
pub(crate) fn parse_cookie(cookie_header: &str) -> Vec<(String, String)> {
    cookie_header
        .split(';')
        .filter_map(|pair| {
            let mut parts = pair.trim().splitn(2, '=');
            let name = parts.next()?.trim();
            let value = parts.next().unwrap_or("").trim();
            if name.is_empty() {
                None
            } else {
                Some((name.to_string(), value.to_string()))
            }
        })
        .collect()
}

/// Validates whether a given string is a well-formed URL path.
///
/// The path must:
/// - Prefix with an HTTP method (e.g., `get:`, `post:`, etc.)
/// - Start with a `/`
/// - Not contain a query (`?`) or fragment (`#`)
///
/// # Examples
///
/// ```
/// assert!(is_valid_path("get:/users/123"));
/// assert!(is_valid_path("get:/users/{user_id}"));
/// assert!(!is_valid_path("users/123")); // does not start with `/`
/// assert!(!is_valid_path("/users/123?query")); // contains query
/// ```
///
/// # Parameters
/// - `path`: The input string to validate as a path.
///
/// # Returns
/// `true` if the path is valid; `false` otherwise.
pub(crate) fn is_valid_path(path: &str) -> bool {
    if !path.starts_with('/') {
        return false;
    }
    if path.contains('?') || path.contains('#') {
        return false;
    }

    [
        path.starts_with("get:"),
        path.starts_with("post:"),
        path.starts_with("put:"),
        path.starts_with("patch:"),
        path.starts_with("delete:"),
        path.starts_with("connect:"),
        path.starts_with("head:"),
        path.starts_with("options:"),
        path.starts_with("trace:"),
    ]
    .into_iter()
    .any(identity)
}

/// Replaces consecutive forward slashes in a string with a single slash.
///
/// This function is useful for normalizing URL or file paths by collapsing
/// multiple consecutive slashes (`///`) into a single slash (`/`).
///
/// # Examples
///
/// ```
/// let input = "///a//b////c/";
/// let result = collapse_slashes(input);
/// assert_eq!(result, "/a/b/c/");
/// ```
///
/// # Parameters
/// - `input`: The input string to normalize.
///
/// # Returns
/// A new `String` with all consecutive slashes replaced by a single slash.
pub(crate) fn collapse_slashes(input: &str) -> String {
    let re = Regex::new(r"/+").unwrap();
    re.replace_all(input, "/").to_string()
}
