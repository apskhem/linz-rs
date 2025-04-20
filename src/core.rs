use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{
    Arc,
    OnceLock,
};

use bytes::Bytes;
use http::{
    Extensions,
    HeaderMap,
    HeaderName,
    StatusCode,
};
use mime::Mime;
use schemars::gen::*;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::err::*;

pub type EndpointHandler<Q, H, P, C, B, R> = Arc<
    dyn Fn(Request<Q, H, P, C, B>, Extensions) -> Pin<Box<dyn Future<Output = HTTPResponse<R>> + Send>> + Send + Sync,
>;

pub type SecurityHandler = Arc<
    dyn Fn(&http::Request<Bytes>, &mut Extensions) -> Pin<Box<dyn Future<Output = Result<(), SecurityError>> + Send>>
        + Send
        + Sync,
>;

pub trait DataTransferObject: JsonSchema + DeserializeOwned + Serialize + Clone + Send + Sync {
    fn bind() -> PhantomData<Self> {
        PhantomData::<Self>::default()
    }

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
        serde_json::from_slice(payload).map_err(PayloadError::InvalidData)
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

#[derive(Clone)]
pub struct Endpoint<Q, H, P, C, B, R>
where
    Q: DataTransferObject,
    H: DataTransferObject,
    P: DataTransferObject,
    C: DataTransferObject,
    B: DataTransferObject,
    R: DataTransferObject,
{
    pub tags: Vec<oas3::spec::Tag>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub operation_id: String,
    pub parameters: Parameters<Q, H, P, C>,
    pub request_body: PhantomData<B>,
    pub responses: Responses<R>,
    pub deprecated: Option<bool>,
    pub security: Vec<Security>,
    pub handler: EndpointHandler<Q, H, P, C, B, R>,
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

fn noop_handler<Q, H, P, C, B>() -> EndpointHandler<Q, H, P, C, B, ()>
where
    Q: DataTransferObject,
    H: DataTransferObject,
    P: DataTransferObject,
    C: DataTransferObject,
    B: DataTransferObject,
{
    Arc::new(|_, _| Box::pin(async { HTTPResponse::<()>::default() }))
}

#[derive(Debug, Clone)]
pub struct Request<Q, H, P, C, B>
where
    Q: DataTransferObject,
    H: DataTransferObject,
    P: DataTransferObject,
    C: DataTransferObject,
    B: DataTransferObject,
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
    Q: DataTransferObject,
    H: DataTransferObject,
    P: DataTransferObject,
    C: DataTransferObject,
{
    pub query: Q,
    pub header: H,
    pub path: P,
    pub cookie: C,
}

impl Default for Parameters<(), (), (), ()> {
    fn default() -> Self {
        Self {
            query: Default::default(),
            header: Default::default(),
            path: Default::default(),
            cookie: Default::default(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct HTTPResponse<B: DataTransferObject> {
    pub headers: HeaderMap,
    pub status: StatusCode,
    pub body: Option<B>,
}

impl<B: DataTransferObject> HTTPResponse<B> {
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
pub struct SenderBody<B: DataTransferObject> {
    pub description: Option<String>,
    pub headers: HashMap<HeaderName, String>,
    pub examples: HashMap<String, oas3::spec::Example>,
    pub mime_type: Mime,
    pub body: Option<B>,
}

impl<B: DataTransferObject> Default for SenderBody<B> {
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
pub struct Responses<B: DataTransferObject> {
    pub ok_response: (StatusCode, SenderBody<B>),
    pub other_responses: Vec<(StatusCode, SenderBody<()>)>,
    pub default_response: Option<SenderBody<()>>,
}

/// Represents a security scheme that can be used by the operations.
#[derive(Clone)]
pub struct Security {
    /// The name of the header, query or cookie parameter to be used.
    pub name: String,
    /// A description for security scheme.
    pub description: Option<String>,
    /// The type of the security scheme.
    pub r#type: security::SecurityType,
    /// A handler for implementing and performing authentication.
    pub handler: SecurityHandler,
}

pub mod security {
    use std::collections::HashMap;

    use serde::{
        Deserialize,
        Serialize,
    };
    use url::Url;

    /// Represents the type of the security scheme.
    #[derive(Debug, Clone)]
    pub enum SecurityType {
        Http {
            /// The name of the HTTP Authentication scheme to be used
            /// in the Authorization header as defined in RFC7235.
            scheme: AuthenticationScheme,
            /// A hint to the client to identify how the bearer token is formatted.
            /// Bearer tokens are usually generated by an authorization server,
            /// so this information is primarily for documentation purposes.
            /// 
            /// E.g., "JWT"
            bearer_format: Option<String>,
        },
        ApiKey {
            /// The name of the header, query or cookie parameter to be used.
            name: String,
            /// The location of the API key.
            r#in: In,
        },
        OAuth2 {
            /// An object containing configuration information for the flow types supported.
            flows: OAuth2SecurityFlows,
        },
        OpenIdConnect {
            /// Well-known URL to discover the [[OpenID-Connect-Discovery]] provider metadata.
            open_id_connect_url: Url,
        },
        MutualTLS {},
    }

    /// Represents authentication scheme registered in IANA Authentication Scheme registry.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum AuthenticationScheme {
        Basic,
        Bearer,
        Concealed,
        Digest,
        DPoP,
        GNAP,
        HOBA,
        Mutual,
        Negotiate,
        OAuth,
        PrivateToken,
        #[serde(rename = "SCRAM-SHA-1")]
        ScramSha1,
        #[serde(rename = "SCRAM-SHA-256")]
        ScramSha256,
        #[serde(rename = "vapid")]
        Vapid,
    }

    /// Represents the location of the API key.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum In {
        Query,
        Header,
        Cookie,
    }

    #[derive(Debug, Clone, Default)]
    pub struct OAuth2SecurityFlows {
        pub implicit: Option<ImplicitOAuth2SecurityFlows>,
        pub password: Option<PasswordOAuth2SecurityFlows>,
        pub client_credentials: Option<ClientCredentialsOAuth2SecurityFlows>,
        pub authorization_code: Option<AuthorizationCodeOAuth2SecurityFlows>,
    }

    #[derive(Debug, Clone)]
    pub struct ImplicitOAuth2SecurityFlows {
        pub authorization_url: Url,
        pub refrest_url: Option<Url>,
        pub scopes: HashMap<String, String>,
    }

    #[derive(Debug, Clone)]
    pub struct PasswordOAuth2SecurityFlows {
        pub token_url: Url,
        pub refrest_url: Option<Url>,
        pub scopes: HashMap<String, String>,
    }

    #[derive(Debug, Clone)]
    pub struct ClientCredentialsOAuth2SecurityFlows {
        pub token_url: Url,
        pub refrest_url: Option<Url>,
        pub scopes: HashMap<String, String>,
    }

    #[derive(Debug, Clone)]
    pub struct AuthorizationCodeOAuth2SecurityFlows {
        pub authorization_url: Url,
        pub token_url: Url,
        pub refrest_url: Option<Url>,
        pub scopes: HashMap<String, String>,
    }
}

// blank type
impl DataTransferObject for () {}
