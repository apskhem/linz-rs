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

    pub fn into_response() {
        
    }
}
