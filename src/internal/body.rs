use bytes::Bytes;
use http::StatusCode;
use serde_json::{
    Map,
    Value,
};

use crate::endpoint::{
    BodyKind,
    File,
};
use crate::err::BodyParserError;
use crate::internal::files::FileStore;
use crate::internal::{
    multipart,
    utils,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentType {
    pub essence: String,
    pub params: Vec<(String, String)>,
}

impl ContentType {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+.^_`|~-".contains(&b))
}

/// Parses a `Content-Type` header value according to RFC 7231.
pub(crate) fn parse_content_type(header: &str) -> Result<ContentType, String> {
    let mut parts = header.split(';');
    let essence = parts.next().unwrap_or_default().trim();

    let valid = essence.split_once('/').is_some_and(|(t, s)| is_token(t) && is_token(s));
    if !valid {
        return Err("invalid media type".to_string());
    }

    let mut params = Vec::new();
    for param in parts {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }
        let Some((key, value)) = param.split_once('=') else {
            return Err("invalid parameter format".to_string());
        };
        let key = key.trim();
        if !is_token(key) {
            return Err("invalid parameter format".to_string());
        }
        let value = value.trim();
        let value = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            Some(quoted) => quoted.replace("\\\"", "\"").replace("\\\\", "\\"),
            None => value.to_string(),
        };
        params.push((key.to_ascii_lowercase(), value));
    }

    Ok(ContentType {
        essence: essence.to_ascii_lowercase(),
        params,
    })
}

fn decode_text(bytes: &[u8], charset: Option<&str>) -> Result<String, String> {
    match charset.map(|c| c.trim().to_ascii_lowercase()).as_deref() {
        None | Some("utf-8") | Some("utf8") => Ok(String::from_utf8_lossy(bytes).into_owned()),
        Some("us-ascii") | Some("ascii") | Some("latin1") | Some("iso-8859-1") | Some("binary") => {
            Ok(bytes.iter().map(|&b| b as char).collect())
        }
        Some(other) => Err(format!("Unknown encoding: {other}")),
    }
}

/// A request body parsed into the intermediate JSON representation.
#[derive(Debug, Clone)]
pub(crate) struct ParsedBody {
    pub value: Value,
    pub files: FileStore,
    /// Whether the values came from a textual form encoding and may be coerced.
    pub textual: bool,
}

/// Parses a request body according to its `Content-Type`. Empty bodies yield `None`. When a
/// form field repeats, the last occurrence is used.
pub(crate) fn parse_body(body: &Bytes, content_type_header: &str) -> Result<Option<ParsedBody>, BodyParserError> {
    if body.is_empty() {
        return Ok(None);
    }

    let content_type =
        parse_content_type(content_type_header).map_err(|err| BodyParserError::new(StatusCode::BAD_REQUEST, err))?;
    let charset = content_type.param("charset");
    let mut files = FileStore::default();

    let (value, textual) = match content_type.essence.as_str() {
        "application/json" => {
            let text = decode_text(body, charset).map_err(|e| BodyParserError::new(StatusCode::BAD_REQUEST, e))?;
            let value = serde_json::from_str(&text)
                .map_err(|e| BodyParserError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
            (value, false)
        }
        "multipart/form-data" => {
            let boundary = content_type
                .param("boundary")
                .map(|b| b.trim().trim_matches(|c| c == '"' || c == '\''))
                .filter(|b| !b.is_empty())
                .ok_or_else(|| BodyParserError::new(StatusCode::BAD_REQUEST, "Cannot find multipart boundary"))?;

            let mut pairs: Vec<(String, Value)> = Vec::new();
            for part in multipart::parse(body, boundary) {
                let Some(name) = part.name else {
                    continue;
                };

                let value = if part.filename.is_some() {
                    Value::String(files.insert(File {
                        name: part.filename,
                        content_type: part.content_type,
                        data: part.data,
                    }))
                } else {
                    let charset = part
                        .content_type
                        .as_deref()
                        .and_then(|ct| parse_content_type(ct).ok())
                        .and_then(|ct| ct.param("charset").map(str::to_string));
                    let text = decode_text(&part.data, charset.as_deref())
                        .map_err(|e| BodyParserError::new(StatusCode::BAD_REQUEST, e))?;
                    Value::String(text)
                };

                pairs.push((name, value));
            }

            (Value::Object(pairs.into_iter().collect::<Map<_, _>>()), true)
        }
        "application/x-www-form-urlencoded" => {
            let text = decode_text(body, charset).map_err(|e| BodyParserError::new(StatusCode::BAD_REQUEST, e))?;
            let pairs = url::form_urlencoded::parse(text.as_bytes()).into_owned();
            (utils::pairs_to_json(pairs), true)
        }
        "application/octet-stream" => {
            let placeholder = files.insert(File {
                name: None,
                content_type: Some(content_type.essence.clone()),
                data: body.clone(),
            });
            (Value::String(placeholder), false)
        }
        other => {
            return Err(BodyParserError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("'{other}' content type is not supported"),
            ));
        }
    };

    Ok(Some(ParsedBody { value, files, textual }))
}

/// An encoded response body.
pub(crate) struct Encoded {
    pub bytes: Vec<u8>,
    /// Overrides the `Content-Type` of the body kind, e.g. to append a boundary.
    pub content_type: Option<String>,
}

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Encodes a validated response value according to the body kind.
pub(crate) fn encode_body(kind: BodyKind, value: &Value, files: &FileStore) -> Result<Encoded, String> {
    let plain = |bytes: Vec<u8>| {
        Encoded {
            bytes,
            content_type: None,
        }
    };

    match kind {
        BodyKind::Json => serde_json::to_vec(value).map(plain).map_err(|e| e.to_string()),
        BodyKind::Text | BodyKind::Html => {
            match value {
                Value::String(s) => Ok(plain(s.as_bytes().to_vec())),
                other => Ok(plain(scalar_to_string(other).into_bytes())),
            }
        }
        BodyKind::OctetStream => {
            match value {
                Value::String(s) => {
                    Ok(plain(
                        files
                            .get(s)
                            .map(|f| f.data.to_vec())
                            .unwrap_or_else(|| s.as_bytes().to_vec()),
                    ))
                }
                _ => Err("an octet-stream body must be a `File` or a string".to_string()),
            }
        }
        BodyKind::UrlEncoded => {
            let Value::Object(map) = value else {
                return Err("a url-encoded body must be an object".to_string());
            };
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for (key, item) in map {
                match item {
                    Value::Array(items) => {
                        for item in items {
                            serializer.append_pair(key, &scalar_to_string(item));
                        }
                    }
                    other => {
                        serializer.append_pair(key, &scalar_to_string(other));
                    }
                }
            }
            Ok(plain(serializer.finish().into_bytes()))
        }
        BodyKind::FormData => {
            let Value::Object(map) = value else {
                return Err("a multipart body must be an object".to_string());
            };
            let mut items = Vec::new();
            for (key, item) in map {
                let values: Vec<&Value> = match item {
                    Value::Array(items) => items.iter().collect(),
                    other => vec![other],
                };
                for value in values {
                    let encoded = match value {
                        Value::String(s) => {
                            match files.get(s) {
                                Some(file) => {
                                    multipart::EncodeItem::File {
                                        filename: file.name.as_deref(),
                                        content_type: file.content_type.as_deref(),
                                        data: &file.data,
                                    }
                                }
                                None => multipart::EncodeItem::Text(s.clone()),
                            }
                        }
                        other => multipart::EncodeItem::Text(scalar_to_string(other)),
                    };
                    items.push((key.clone(), encoded));
                }
            }
            let boundary = multipart::generate_boundary();
            Ok(Encoded {
                bytes: multipart::encode(&items, &boundary),
                content_type: Some(format!("{}; boundary={boundary}", kind.mime_type())),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_types() {
        let ct = parse_content_type("Multipart/Form-Data; boundary=\"abc\"; charset=utf-8").unwrap();
        assert_eq!(ct.essence, "multipart/form-data");
        assert_eq!(ct.param("boundary"), Some("abc"));
        assert_eq!(ct.param("charset"), Some("utf-8"));
        assert!(parse_content_type("").is_err());
        assert!(parse_content_type("json").is_err());
    }

    #[test]
    fn parses_bodies() {
        assert!(parse_body(&Bytes::new(), "").unwrap().is_none());

        let json = parse_body(&Bytes::from_static(b"{\"a\":1}"), "application/json")
            .unwrap()
            .unwrap();
        assert_eq!(json.value, serde_json::json!({"a": 1}));

        let form = parse_body(
            &Bytes::from_static(b"a=1&a=2&b=x+y"),
            "application/x-www-form-urlencoded",
        )
        .unwrap()
        .unwrap();
        assert_eq!(form.value, serde_json::json!({"a": "2", "b": "x y"}));

        let err = parse_body(&Bytes::from_static(b"x"), "text/csv").unwrap_err();
        assert_eq!(err.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let err = parse_body(&Bytes::from_static(b"{"), "application/json").unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }
}
