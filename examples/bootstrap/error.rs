//! Domain errors and how they are rendered as HTTP errors.

use http::StatusCode;
use linz_rs::err::JsonApiErrorSource;
use linz_rs::{
    ApiError,
    JsonApiErrorItem,
};

/// The errors of the application. Handlers return them with `?`, and the `From` impl below
/// turns them into JSON:API error responses.
#[derive(Debug)]
pub enum AppError {
    /// The resource does not exist.
    NotFound(&'static str),
    /// The request conflicts with the current state, caused by the body field at `pointer`.
    Conflict {
        code: &'static str,
        message: String,
        pointer: &'static str,
    },
    /// An upload exceeds the configured limit.
    PayloadTooLarge { limit: usize },
}

impl From<AppError> for ApiError {
    fn from(err: AppError) -> Self {
        match err {
            AppError::NotFound(resource) => ApiError::detail(StatusCode::NOT_FOUND, format!("{resource} not found")),
            AppError::Conflict { code, message, pointer } => {
                ApiError::new(
                    StatusCode::CONFLICT,
                    JsonApiErrorItem {
                        code: Some(code.to_string()),
                        detail: Some(message),
                        source: Some(JsonApiErrorSource {
                            pointer: Some(pointer.to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )
            }
            AppError::PayloadTooLarge { limit } => {
                ApiError::detail(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!("The file exceeds the limit of {limit} bytes"),
                )
            }
        }
    }
}
