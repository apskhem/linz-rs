use http::{HeaderMap, HeaderValue};
use serde_json::Value;

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
