use std::collections::HashMap;

use bytes::Bytes;

use crate::internal::utils;

#[derive(Debug, Clone)]
pub(crate) struct Part {
    pub name: Option<String>,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Bytes,
}

/// Parses a `multipart/form-data` body.
pub(crate) fn parse(body: &Bytes, boundary: &str) -> Vec<Part> {
    let delimiter = format!("--{boundary}");
    let delimiter = delimiter.as_bytes();
    let mut parts = Vec::new();

    let Some(mut cursor) = find(body, delimiter, 0) else {
        return parts;
    };

    loop {
        let after_delimiter = cursor + delimiter.len();

        // closing delimiter
        if body[after_delimiter..].starts_with(b"--") {
            break;
        }

        let Some(next) = find(body, delimiter, after_delimiter) else {
            break;
        };

        let mut start = after_delimiter;
        if body[start..].starts_with(b"\r\n") {
            start += 2;
        }

        let mut end = next;
        if end >= start + 2 && &body[end - 2..end] == b"\r\n" {
            end -= 2;
        }

        if let Some(part) = parse_part(body.slice(start..end.max(start))) {
            parts.push(part);
        }

        cursor = next;
    }

    parts
}

fn parse_part(raw: Bytes) -> Option<Part> {
    let header_end = find(&raw, b"\r\n\r\n", 0);
    let (head, data) = match header_end {
        Some(index) => (&raw[..index], raw.slice(index + 4..)),
        None if raw.starts_with(b"\r\n") => (&raw[..0], raw.slice(2..)),
        None => return None,
    };

    let headers: HashMap<String, String> = String::from_utf8_lossy(head)
        .split("\r\n")
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let key = key.trim();
            (!key.is_empty()).then(|| (key.to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();

    let disposition = parse_content_disposition(headers.get("content-disposition").map(String::as_str).unwrap_or(""));

    Some(Part {
        name: disposition.get("name").cloned(),
        filename: disposition.get("filename").cloned(),
        content_type: headers.get("content-type").cloned(),
        data,
    })
}

fn parse_content_disposition(value: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut parts = value.split(';').map(str::trim);

    if let Some(kind) = parts.next().filter(|k| !k.is_empty()) {
        result.insert("type".to_string(), kind.to_ascii_lowercase());
    }

    for part in parts {
        if let Some((key, value)) = part.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);

            if !key.is_empty() && !value.is_empty() {
                result.insert(key, value.to_string());
            }
        }
    }

    result
}

pub(crate) fn generate_boundary() -> String {
    format!("----------------------{}", utils::random_hex(12))
}

pub(crate) enum EncodeItem<'a> {
    Text(String),
    File {
        filename: Option<&'a str>,
        content_type: Option<&'a str>,
        data: &'a [u8],
    },
}

/// Encodes fields into a `multipart/form-data` body.
pub(crate) fn encode(items: &[(String, EncodeItem<'_>)], boundary: &str) -> Vec<u8> {
    let mut out = Vec::new();

    for (name, item) in items {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());

        match item {
            EncodeItem::Text(text) => {
                out.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
                out.extend_from_slice(text.as_bytes());
            }
            EncodeItem::File {
                filename,
                content_type,
                data,
            } => {
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\n",
                        filename.unwrap_or(name)
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(
                    format!(
                        "Content-Type: {}\r\n\r\n",
                        content_type.unwrap_or("application/octet-stream")
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(data);
            }
        }

        out.extend_from_slice(b"\r\n");
    }

    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }

    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|index| index + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let boundary = generate_boundary();
        let body = encode(
            &[
                ("field".to_string(), EncodeItem::Text("value".to_string())),
                (
                    "file".to_string(),
                    EncodeItem::File {
                        filename: Some("a.txt"),
                        content_type: Some("text/plain"),
                        data: b"line1\r\nline2",
                    },
                ),
            ],
            &boundary,
        );

        let parts = parse(&Bytes::from(body), &boundary);

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name.as_deref(), Some("field"));
        assert_eq!(parts[0].filename, None);
        assert_eq!(&parts[0].data[..], b"value");
        assert_eq!(parts[1].name.as_deref(), Some("file"));
        assert_eq!(parts[1].filename.as_deref(), Some("a.txt"));
        assert_eq!(parts[1].content_type.as_deref(), Some("text/plain"));
        assert_eq!(&parts[1].data[..], b"line1\r\nline2");
    }
}
