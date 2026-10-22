use std::collections::hash_map::RandomState;
use std::hash::{
    BuildHasher,
    Hasher,
};
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};
use std::sync::OnceLock;

use http::{
    HeaderMap,
    HeaderValue,
};
use regex::Regex;
use serde_json::{
    Map,
    Value,
};

use crate::core::HttpMethod;

/// Converts request headers into a JSON object. Header names are lower-cased and
/// repeated headers are joined with `, `.
pub(crate) fn headers_to_json(headers: &HeaderMap<HeaderValue>) -> Value {
    let mut map = Map::new();

    for (key, value) in headers.iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };

        match map.get_mut(key.as_str()) {
            Some(Value::String(existing)) => {
                existing.push_str(", ");
                existing.push_str(value);
            }
            _ => {
                map.insert(key.to_string(), Value::String(value.to_string()));
            }
        }
    }

    Value::Object(map)
}

/// Converts a query string into a JSON object. With `multi_value` each value is an
/// array of all occurrences, otherwise only the last value is used.
pub(crate) fn query_to_json(query: Option<&str>, multi_value: bool) -> Value {
    pairs_to_json(
        url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()).into_owned(),
        multi_value,
    )
}

pub(crate) fn pairs_to_json(pairs: impl IntoIterator<Item = (String, String)>, multi_value: bool) -> Value {
    let mut merged: Vec<(String, Vec<Value>)> = Vec::new();

    for (key, value) in pairs {
        match merged.iter_mut().find(|(k, _)| *k == key) {
            Some((_, values)) => values.push(Value::String(value)),
            None => merged.push((key, vec![Value::String(value)])),
        }
    }

    Value::Object(
        merged
            .into_iter()
            .map(|(key, mut values)| {
                if multi_value {
                    (key, Value::Array(values))
                } else {
                    (key, values.pop().unwrap_or(Value::Null))
                }
            })
            .collect(),
    )
}

/// Parses a `Cookie` header string into key-value pairs according to RFC 6265.
///
/// Values are percent-decoded and pairs without a `=` are treated as having an empty value.
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
                let value = percent_encoding::percent_decode_str(value).decode_utf8_lossy();
                Some((name.to_string(), value.into_owned()))
            }
        })
        .collect()
}

pub(crate) fn cookies_to_json(cookie_header: &str) -> Value {
    Value::Object(
        parse_cookie(cookie_header)
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect(),
    )
}

/// Splits a `method:path` key, e.g. `get:/users/:id`, into its method and path.
pub(crate) fn split_method_path(method_path: &str) -> Result<(HttpMethod, String), String> {
    let Some((method, path)) = method_path.split_once(':') else {
        return Err(format!("'{method_path}' is not a valid `method:path` key"));
    };

    let Some(method) = HttpMethod::parse(method) else {
        return Err(format!("Invalid method \"{method}\" for path {path}"));
    };

    if !path.starts_with('/') || path.contains('?') || path.contains('#') {
        return Err(format!("'{path}' is not a valid path"));
    }

    Ok((method, path.to_string()))
}

/// Replaces consecutive forward slashes in a string with a single slash.
pub(crate) fn clean_path(path: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/+").unwrap())
        .replace_all(path, "/")
        .into_owned()
}

/// Converts `:param` path segments into the OpenAPI/axum `{param}` notation and
/// returns the collected parameter names.
pub(crate) fn convert_path_params(path: &str) -> (String, Vec<String>) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r":([^/]+)").unwrap());

    let new_path = re.replace_all(&clean_path(path), "{$1}").into_owned();
    let params = re.captures_iter(path).map(|c| c[1].to_string()).collect();

    (new_path, params)
}

/// Converts an identifier like `echoPathSimple` or `echo-path simple` into `EchoPathSimple`.
pub(crate) fn pascal_case(input: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;

    for ch in input.chars() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        prev_lower = ch.is_lowercase() || ch.is_numeric();
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }

    words
        .into_iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
                None => String::new(),
            }
        })
        .collect()
}

/// Generates `bytes` random bytes encoded as lower-case hex. Not suitable for cryptography.
pub(crate) fn random_hex(bytes: usize) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut out = String::with_capacity(bytes * 2);

    while out.len() < bytes * 2 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        for byte in hasher.finish().to_le_bytes() {
            if out.len() < bytes * 2 {
                out.push_str(&format!("{byte:02x}"));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(input: &str, path: &str, params: &[&str]) {
        assert_eq!(
            convert_path_params(input),
            (
                path.to_string(),
                params.iter().map(|s| s.to_string()).collect::<Vec<_>>()
            )
        );
    }

    #[test]
    fn convert_path_params_cases() {
        check("/users/:userId", "/users/{userId}", &["userId"]);
        check(
            "/users/:userId/posts/:postId",
            "/users/{userId}/posts/{postId}",
            &["userId", "postId"],
        );
        check("/users/posts", "/users/posts", &[]);
        check(
            "/users/:userId/posts/:postId/",
            "/users/{userId}/posts/{postId}/",
            &["userId", "postId"],
        );
        check(
            "/users///:userId//posts/:postId",
            "/users/{userId}/posts/{postId}",
            &["userId", "postId"],
        );
        check("/products/:product-id", "/products/{product-id}", &["product-id"]);
        check(
            "/api/:version1/:id123",
            "/api/{version1}/{id123}",
            &["version1", "id123"],
        );
        check("", "", &[]);
        check("/:onlyParam", "/{onlyParam}", &["onlyParam"]);
        check(
            "/path-with-special_chars-:param123",
            "/path-with-special_chars-{param123}",
            &["param123"],
        );
        check(
            "/api/v1/users/:userId/orders/:orderId/items/:itemId",
            "/api/v1/users/{userId}/orders/{orderId}/items/{itemId}",
            &["userId", "orderId", "itemId"],
        );
    }

    #[test]
    fn collapses_slashes() {
        assert_eq!(clean_path("///a//b////c/"), "/a/b/c/");
    }

    #[test]
    fn pascal_cases_operation_ids() {
        assert_eq!(pascal_case("echoPathSimple"), "EchoPathSimple");
        assert_eq!(pascal_case("get-user by_id"), "GetUserById");
        assert_eq!(pascal_case("HTTPServer"), "Httpserver");
    }

    #[test]
    fn parses_cookies() {
        assert_eq!(
            parse_cookie("a=1; b=hello%20world; c; =x; d=e=f"),
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "hello world".to_string()),
                ("c".to_string(), "".to_string()),
                ("d".to_string(), "e=f".to_string()),
            ]
        );
    }

    #[test]
    fn splits_method_paths() {
        assert_eq!(
            split_method_path("get:/users/:id").unwrap(),
            (HttpMethod::Get, "/users/:id".to_string())
        );
        assert!(split_method_path("fetch:/users").is_err());
        assert!(split_method_path("get:users").is_err());
        assert!(split_method_path("/users").is_err());
    }

    #[test]
    fn merges_query_values() {
        assert_eq!(
            query_to_json(Some("a=1&b=2&a=3"), false),
            serde_json::json!({"a": "3", "b": "2"})
        );
        assert_eq!(
            query_to_json(Some("a=1&b=2&a=3"), true),
            serde_json::json!({"a": ["1", "3"], "b": ["2"]})
        );
    }
}
