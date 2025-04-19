use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use http::{HeaderMap, HeaderName, StatusCode};
use mime::Mime;
use schemars::{gen::*, JsonSchema};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::err::*;

type Handler<Q, H, P, C, B, R> =
    Arc<dyn Fn(Request<Q, H, P, C, B>) -> Pin<Box<dyn Future<Output = HTTPResponse<R>> + Send>> + Send + Sync>;

pub trait Payload: JsonSchema + DeserializeOwned + Serialize {
    fn jsonschema_validator() -> &'static jsonschema::Validator {
        static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();

        VALIDATOR.get_or_init(|| {
            let mut generator = SchemaGenerator::new(SchemaSettings::openapi3());

            let schema = Self::json_schema(&mut generator);
            let json = serde_json::to_value(&schema).unwrap();
            let validator = jsonschema::validator_for(&json).unwrap();
    
            validator
        })
    }

    fn validate_and_transform(payload: &[u8]) -> Result<Self, ValidationError> {
        let payload = serde_json::from_slice(payload).map_err(ValidationError::InvalidJSON)?;

        let err_results: Vec<_> = Self::jsonschema_validator()
            .iter_errors(&payload)
            .map(ValidationFieldError::from)
            .collect();

        if !err_results.is_empty() {
            return Err(ValidationError::FailedValidating(err_results));
        }

        Self::deserialize(payload).map_err(ValidationError::FailedDeserializing)
    }
}

pub struct Endpoint<Q, H, P, C, B, R: Payload> {
    pub tags: Vec<oas3::spec::Tag>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub operation_id: String,
    pub parameters: Parameters<Q, H, P, C>,
    pub request_body: Option<B>,
    pub responses: Responses<R>,
    pub deprecated: Option<bool>,
    pub security: Vec<Security>,
    pub handler: Handler<Q, H, P, C, B, R>,
}

/* impl<Q, H, P, C, B> Default for Endpoint<Q, H, P, C, B> {
    fn default() -> Self {
        Self {
            tags: Default::default(),
            summary: Default::default(),
            description: Default::default(),
            operation_id: Default::default(),
            parameters: Parameters {
                query: (),
                header: (),
                path: (),
                cookie: ()
            },
            request_body: Default::default(),
            responses: Default::default(),
            deprecated: Default::default(),
            security: Default::default(),
            handler: noop_handler(),
        }
    }
} */

fn noop_handler<Q, H, P, C, B, R: Payload>() -> Handler<Q, H, P, C, B, ()> {
    Arc::new(|_| Box::pin(async {
        HTTPResponse::<()>::default()
    }))
}

pub struct Request<Q, H, P, C, B> {
    pub queries: Q,
    pub headers: H,
    pub params: P,
    pub cookies: C,
    pub body: B,
}

#[derive(Default)]
pub struct Parameters<Q, H, P, C> {
    pub query: Q,
    pub header: H,
    pub path: P,
    pub cookie: C,
}

#[derive(Debug, Default)]
pub struct HTTPResponse<B: Payload> {
    pub headers: HeaderMap,
    pub status: StatusCode,
    pub body: Option<B>
}

#[derive(Debug)]
pub struct SenderBody<B: Payload> {
    pub description: Option<String>,
    pub headers: HashMap<HeaderName, String>,
    pub examples: HashMap<String, oas3::spec::Example>,
    pub mime_type: Mime,
    pub body: Option<B>
}

impl<B: Payload> Default for SenderBody<B> {
    fn default() -> Self {
        Self {
            description: Default::default(),
            headers: Default::default(),
            examples: Default::default(),
            mime_type: mime::APPLICATION_JSON,
            body: None
        }
    }
}

#[derive(Debug, Default)]
pub struct Responses<B: Payload> {
    pub ok_response: (StatusCode, SenderBody<B>),
    pub other_responses: (StatusCode, SenderBody<()>),
    pub default_response: Option<SenderBody<()>>
}

#[derive(Debug, Default)]
pub struct Security {}

impl Payload for () { }