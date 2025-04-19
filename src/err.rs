#[derive(Debug)]
pub struct ValidationFieldError {
    pub kind: jsonschema::error::ValidationErrorKind,
    pub path: String,
}

impl From<jsonschema::ValidationError<'_>> for ValidationFieldError {
    fn from(value: jsonschema::ValidationError<'_>) -> Self {
        Self { kind: value.kind, path: value.instance_path.to_string() }
    }
}

#[derive(Debug)]
pub enum ValidationError {
    InvalidJSON(serde_json::Error),
    FailedValidating(Vec<ValidationFieldError>),
    FailedDeserializing(serde_json::Error),
}
