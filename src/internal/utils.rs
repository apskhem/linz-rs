use axum::body::Body;
use axum::response::Response;
use http::{
    header,
    HeaderMap,
    HeaderValue,
    StatusCode,
};
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

pub(crate) fn handle_internal_error(msg: impl ToString) -> Response<Body> {
    let mut json_body = serde_json::Map::with_capacity(2);

    json_body.insert("code".into(), StatusCode::INTERNAL_SERVER_ERROR.as_u16().into());
    json_body.insert("message".into(), msg.to_string().into());

    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, mime::APPLICATION_JSON.to_string())
        .body(Body::from(serde_json::Value::Object(json_body).to_string()))
        .unwrap()
}
