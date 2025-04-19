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

type Handler<Q, H, P, C, B, R> = Arc<
    dyn Fn(Request<Q, H, P, C, B>) -> Pin<Box<dyn Future<Output = HTTPResponse<R>> + Send>>
        + Send
        + Sync,
>;

pub trait Payload: JsonSchema + DeserializeOwned + Serialize + Clone + Send + Sync {
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

    fn decoding_content_type() -> Mime {
        mime::APPLICATION_JSON
    }

    fn content_type_decoder(payload: &[u8]) -> Result<serde_json::Value, PayloadError> {
        serde_json::from_slice(payload).map_err(PayloadError::InvalidJSON)
    }

    fn validate_json_payload(payload: &serde_json::Value) -> Result<(), PayloadError> {
        let err_results: Vec<_> = Self::jsonschema_validator()
            .iter_errors(payload)
            .map(ValidationFieldError::from)
            .collect();

        if !err_results.is_empty() {
            return Err(PayloadError::FailedValidating(err_results));
        }

        return Ok(());
    }

    fn validate_and_transform(payload: &[u8]) -> Result<Self, PayloadError> {
        let payload = Self::content_type_decoder(payload)?;

        Self::validate_json_payload(&payload)?;

        Self::deserialize(payload).map_err(PayloadError::FailedDeserializing)
    }

    fn encoding_content_type() -> Mime {
        mime::APPLICATION_JSON
    }

    fn content_type_encoder(self) -> Result<serde_json::Value, PayloadError> {
        serde_json::to_value(self).map_err(PayloadError::FailedSerializing)
    }

    fn into_bytes(self) -> Result<Vec<u8>, PayloadError> {
        let payload = self.content_type_encoder()?;

        Ok(payload.to_string().as_bytes().to_vec())
    }
}

pub trait QueryPayload: Payload {
    fn content_type_decoder(payload: &[u8]) -> Result<serde_json::Value, PayloadError> {
        Ok(serde_qs::from_bytes(payload).unwrap())
    }
}

pub trait HeaderPayload: Payload {}

pub trait PathPayload: Payload {}

pub trait CookiePayload: Payload {}

#[derive(Clone)]
pub struct Endpoint<Q, H, P, C, B, R>
where
    Q: QueryPayload,
    H: HeaderPayload,
    P: PathPayload,
    C: CookiePayload,
    B: Payload,
    R: Payload,
{
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

impl Default for Endpoint<(), (), (), (), (), ()> {
    fn default() -> Self {
        Self {
            tags: Default::default(),
            summary: Default::default(),
            description: Default::default(),
            operation_id: Default::default(),
            parameters: Parameters::default(),
            request_body: Default::default(),
            responses: Default::default(),
            deprecated: Default::default(),
            security: Default::default(),
            handler: noop_handler(),
        }
    }
}

pub trait IntoEndpoint<Q, H, P, C, B, R>
where
    Q: QueryPayload,
    H: HeaderPayload,
    P: PathPayload,
    C: CookiePayload,
    B: Payload,
    R: Payload,
{
    fn get_endpoint() -> Endpoint<Q, H, P, C, B, R>;
}

fn noop_handler<Q, H, P, C, B>() -> Handler<Q, H, P, C, B, ()>
where
    Q: QueryPayload,
    H: HeaderPayload,
    P: PathPayload,
    C: CookiePayload,
    B: Payload,
{
    Arc::new(|_| Box::pin(async { HTTPResponse::<()>::default() }))
}

#[derive(Debug, Clone)]
pub struct Request<Q, H, P, C, B>
where
    Q: QueryPayload,
    H: HeaderPayload,
    P: PathPayload,
    C: CookiePayload,
    B: Payload,
{
    pub queries: Option<Q>,
    pub headers: Option<H>,
    pub params: Option<P>,
    pub cookies: Option<C>,
    pub body: Option<B>,
}

#[derive(Debug, Clone)]
pub struct Parameters<Q, H, P, C>
where
    Q: QueryPayload,
    H: HeaderPayload,
    P: PathPayload,
    C: CookiePayload,
{
    pub query: Option<Q>,
    pub header: Option<H>,
    pub path: Option<P>,
    pub cookie: Option<C>,
}

impl Default for Parameters<(), (), (), ()> {
    fn default() -> Self {
        Self {
            query: None,
            header: None,
            path: None,
            cookie: None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct HTTPResponse<B: Payload> {
    pub headers: HeaderMap,
    pub status: StatusCode,
    pub body: Option<B>,
}

impl<B: Payload> HTTPResponse<B> {
    fn send_status(status: StatusCode) -> Self {
        Self {
            status,
            headers: Default::default(),
            body: None,
        }
    }

    fn without_body(status: StatusCode, headers: Option<HeaderMap>) -> Self {
        Self {
            status,
            headers: headers.unwrap_or_default(),
            body: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SenderBody<B: Payload> {
    pub description: Option<String>,
    pub headers: HashMap<HeaderName, String>,
    pub examples: HashMap<String, oas3::spec::Example>,
    pub mime_type: Mime,
    pub body: Option<B>,
}

impl<B: Payload> Default for SenderBody<B> {
    fn default() -> Self {
        Self {
            description: Default::default(),
            headers: Default::default(),
            examples: Default::default(),
            mime_type: mime::APPLICATION_JSON,
            body: None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Responses<B: Payload> {
    pub ok_response: (StatusCode, SenderBody<B>),
    pub other_responses: (StatusCode, SenderBody<()>),
    pub default_response: Option<SenderBody<()>>,
}

#[derive(Debug, Default, Clone)]
pub struct Security {}

// blank type
impl Payload for () {}
impl QueryPayload for () {}
impl PathPayload for () {}
impl HeaderPayload for () {}
impl CookiePayload for () {}
