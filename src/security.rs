//! Security schemes: [`Security`], [`AppliedSecurity`] and the token extraction per [`SecurityType`].

use core::fmt;
use std::future::Future;
use std::sync::Arc;

use http::{
    Extensions,
    HeaderMap,
    Method,
    Uri,
    header,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    Value,
    json,
};
use url::Url;

use crate::core::BoxFuture;
use crate::err::*;
use crate::internal::utils;

/// The input of a [`SecurityHandler`].
#[derive(Debug, Clone)]
pub struct Credentials {
    /// The token extracted according to the [`SecurityType`]:
    /// - `Http`: the credentials of the `Authorization` header with the matched scheme
    /// - `ApiKey`: the value of the header, query or cookie parameter
    /// - `OAuth2`/`OpenIdConnect`: the `Bearer` token of the `Authorization` header
    /// - `MutualTLS`: `None`
    pub token: Option<String>,
    /// The scopes required by the endpoint, see [`Security::apply`].
    pub scopes: Vec<String>,
    /// The method of the request.
    pub method: Method,
    /// The URI of the request.
    pub uri: Uri,
    /// The headers of the request.
    pub headers: HeaderMap,
}

/// A handler for implementing and performing authentication. On success, it returns
/// objects to attach to [`crate::core::Context::extensions`].
pub type SecurityHandler = Arc<dyn Fn(Credentials) -> BoxFuture<Result<Extensions, ApiError>> + Send + Sync>;

/// Represents a security scheme that can be used by the operations.
#[derive(Clone)]
pub struct Security {
    /// The name of the security scheme in the spec.
    pub name: String,
    /// The type of the security scheme.
    pub r#type: SecurityType,
    /// A description for security scheme.
    pub description: Option<String>,
    /// A handler for implementing and performing authentication.
    pub handler: SecurityHandler,
}

impl fmt::Debug for Security {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Security")
            .field("name", &self.name)
            .field("type", &self.r#type)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

impl Security {
    /// Creates a security scheme named `name` (the key in the docs) with its authentication handler.
    pub fn new<F, Fut>(name: impl Into<String>, r#type: SecurityType, handler: F) -> Self
    where
        F: Fn(Credentials) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Extensions, ApiError>> + Send + 'static,
    {
        Self {
            name: name.into(),
            r#type,
            description: None,
            handler: Arc::new(move |credentials| Box::pin(handler(credentials))),
        }
    }

    /// Sets the description in the docs.
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Applies the security to an endpoint with the required scopes.
    pub fn apply(&self, scopes: impl IntoIterator<Item = impl Into<String>>) -> AppliedSecurity {
        AppliedSecurity {
            security: Arc::new(self.clone()),
            scopes: scopes.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns the OpenAPI security scheme object.
    pub fn schema(&self) -> Value {
        let mut schema = match &self.r#type {
            SecurityType::Http { scheme, bearer_format } => {
                let mut s = json!({ "type": "http", "scheme": scheme.as_str() });
                if let Some(format) = bearer_format {
                    s["bearerFormat"] = format.clone().into();
                }
                s
            }
            SecurityType::ApiKey { name, r#in } => json!({ "type": "apiKey", "name": name, "in": r#in.as_str() }),
            SecurityType::OAuth2 { flows } => json!({ "type": "oauth2", "flows": flows.schema() }),
            SecurityType::OpenIdConnect { open_id_connect_url } => {
                json!({ "type": "openIdConnect", "openIdConnectUrl": open_id_connect_url.as_str() })
            }
            SecurityType::MutualTLS => json!({ "type": "mutualTLS" }),
        };

        if let Some(description) = &self.description {
            schema["description"] = description.clone().into();
        }

        schema
    }

    pub(crate) fn extract_token(&self, parts: &http::request::Parts) -> Result<Option<String>, SecurityError> {
        let token = match &self.r#type {
            SecurityType::Http { scheme, .. } => extract_auth_header(&parts.headers, scheme.as_str())?,
            SecurityType::ApiKey { name, r#in } => {
                match r#in {
                    In::Header => {
                        parts
                            .headers
                            .get(name)
                            .ok_or(SecurityError::MissingApiKeyParameter)?
                            .to_str()
                            .map_err(|_| SecurityError::InvalidUtf8HeaderValue)?
                            .to_owned()
                    }
                    In::Query => {
                        let query = parts.uri.query().unwrap_or_default();

                        url::form_urlencoded::parse(query.as_bytes())
                            .find(|(k, _)| k == name)
                            .map(|(_, v)| v.into_owned())
                            .ok_or(SecurityError::MissingApiKeyParameter)?
                    }
                    In::Cookie => {
                        let cookie = parts
                            .headers
                            .get(header::COOKIE)
                            .ok_or(SecurityError::MissingApiKeyParameter)?
                            .to_str()
                            .map_err(|_| SecurityError::InvalidUtf8HeaderValue)?;

                        utils::parse_cookie(cookie)
                            .into_iter()
                            .rev()
                            .find(|(k, _)| k == name)
                            .map(|(_, v)| v)
                            .ok_or(SecurityError::MissingApiKeyParameter)?
                    }
                }
            }
            SecurityType::OAuth2 { .. } | SecurityType::OpenIdConnect { .. } => {
                extract_auth_header(&parts.headers, AuthenticationScheme::Bearer.as_str())?
            }
            SecurityType::MutualTLS => return Ok(None),
        };

        Ok(Some(token))
    }
}

/// A [`Security`] applied to an endpoint with its required scopes.
#[derive(Clone, Debug)]
pub struct AppliedSecurity {
    /// The security scheme.
    pub security: Arc<Security>,
    /// The required scopes.
    pub scopes: Vec<String>,
}

impl AppliedSecurity {
    /// Performs authentication as defined against a request and returns the attached objects.
    pub(crate) async fn authenticate(&self, parts: &http::request::Parts) -> Result<Extensions, ApiError> {
        let token = self.security.extract_token(parts)?;

        (self.security.handler)(Credentials {
            token,
            scopes: self.scopes.clone(),
            method: parts.method.clone(),
            uri: parts.uri.clone(),
            headers: parts.headers.clone(),
        })
        .await
    }
}

impl From<&Security> for AppliedSecurity {
    /// Applies the security without scopes.
    fn from(security: &Security) -> Self {
        security.apply(Vec::<String>::new())
    }
}

impl From<Security> for AppliedSecurity {
    /// Applies the security without scopes.
    fn from(security: Security) -> Self {
        (&security).into()
    }
}

fn extract_auth_header(headers: &HeaderMap, required_scheme: &str) -> Result<String, SecurityError> {
    let auth = headers
        .get(header::AUTHORIZATION)
        .ok_or(SecurityError::MissingAuthorizationHeader)?
        .to_str()
        .map_err(|_| SecurityError::InvalidAuthorizationHeaderValue)?
        .trim();

    let mut parts = auth.splitn(2, char::is_whitespace);
    let scheme = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or(SecurityError::InvalidAuthorizationHeaderValue)?;

    // authentication schemes are case-insensitive (RFC 7235)
    if !scheme.eq_ignore_ascii_case(required_scheme) {
        return Err(SecurityError::InvalidScheme);
    }

    let token = parts
        .next()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or(SecurityError::MissingTokenFromScheme)?;

    Ok(token.to_string())
}

/// Represents the type of the security scheme.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
#[non_exhaustive]
pub enum SecurityType {
    /// HTTP authentication with the `Authorization` header.
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
    /// An API key in a header, query parameter or cookie.
    ApiKey {
        /// The name of the header, query or cookie parameter to be used.
        name: String,
        /// The location of the API key.
        r#in: In,
    },
    /// OAuth 2.0, with a `Bearer` token.
    OAuth2 {
        /// An object containing configuration information for the flow types supported.
        flows: oauth2_flows::Security,
    },
    /// OpenID Connect, with a `Bearer` token.
    OpenIdConnect {
        /// Well-known URL to discover the [[OpenID-Connect-Discovery]] provider metadata.
        open_id_connect_url: Url,
    },
    /// Mutual TLS. No token is extracted.
    MutualTLS,
}

/// Represents authentication scheme registered in IANA Authentication Scheme registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AuthenticationScheme {
    /// RFC 7617
    Basic,
    /// RFC 6750
    Bearer,
    /// RFC 9729
    Concealed,
    /// RFC 7616
    Digest,
    /// RFC 9449
    DPoP,
    /// RFC 9635
    GNAP,
    /// RFC 7486
    HOBA,
    /// RFC 8120
    Mutual,
    /// RFC 4559
    Negotiate,
    /// RFC 5849
    OAuth,
    /// RFC 9577
    PrivateToken,
    /// RFC 7804
    #[serde(rename = "SCRAM-SHA-1")]
    ScramSha1,
    /// RFC 7804
    #[serde(rename = "SCRAM-SHA-256")]
    ScramSha256,
    /// RFC 8292
    #[serde(rename = "vapid")]
    Vapid,
}

impl AuthenticationScheme {
    /// The registered name, e.g. `Bearer`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Bearer => "Bearer",
            Self::Concealed => "Concealed",
            Self::Digest => "Digest",
            Self::DPoP => "DPoP",
            Self::GNAP => "GNAP",
            Self::HOBA => "HOBA",
            Self::Mutual => "Mutual",
            Self::Negotiate => "Negotiate",
            Self::OAuth => "OAuth",
            Self::PrivateToken => "PrivateToken",
            Self::ScramSha1 => "SCRAM-SHA-1",
            Self::ScramSha256 => "SCRAM-SHA-256",
            Self::Vapid => "vapid",
        }
    }
}

impl fmt::Display for AuthenticationScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Represents the location of the API key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum In {
    /// A query parameter.
    Query,
    /// A header.
    Header,
    /// A cookie.
    Cookie,
}

impl In {
    /// The OpenAPI name, e.g. `header`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Header => "header",
            Self::Cookie => "cookie",
        }
    }
}

impl fmt::Display for In {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub mod oauth2_flows {
    //! Configuration of the OAuth 2.0 flows, used for the docs.

    use serde_json::{
        Map,
        Value,
        json,
    };
    use url::Url;

    /// Represents configuration of the supported OAuth Flows.
    #[derive(Debug, Clone, Default)]
    pub struct Security {
        /// Configuration for the OAuth Implicit flow.
        pub implicit: Option<Implicit>,
        /// Configuration for the OAuth Resource Owner Password flow
        pub password: Option<Password>,
        /// Configuration for the OAuth Client Credentials flow.
        pub client_credentials: Option<ClientCredentials>,
        /// Configuration for the OAuth Authorization Code flow.
        pub authorization_code: Option<AuthorizationCode>,
    }

    /// Configuration for the OAuth Implicit flow.
    #[derive(Debug, Clone)]
    pub struct Implicit {
        /// The authorization URL.
        pub authorization_url: Url,
        /// The URL for obtaining refresh tokens.
        pub refresh_url: Option<Url>,
        /// The available scopes, as name and description.
        pub scopes: Vec<(String, String)>,
    }

    /// Configuration for the OAuth Resource Owner Password flow.
    #[derive(Debug, Clone)]
    pub struct Password {
        /// The token URL.
        pub token_url: Url,
        /// The URL for obtaining refresh tokens.
        pub refresh_url: Option<Url>,
        /// The available scopes, as name and description.
        pub scopes: Vec<(String, String)>,
    }

    /// Configuration for the OAuth Client Credentials flow.
    #[derive(Debug, Clone)]
    pub struct ClientCredentials {
        /// The token URL.
        pub token_url: Url,
        /// The URL for obtaining refresh tokens.
        pub refresh_url: Option<Url>,
        /// The available scopes, as name and description.
        pub scopes: Vec<(String, String)>,
    }

    /// Configuration for the OAuth Authorization Code flow.
    #[derive(Debug, Clone)]
    pub struct AuthorizationCode {
        /// The authorization URL.
        pub authorization_url: Url,
        /// The token URL.
        pub token_url: Url,
        /// The URL for obtaining refresh tokens.
        pub refresh_url: Option<Url>,
        /// The available scopes, as name and description.
        pub scopes: Vec<(String, String)>,
    }

    fn flow(
        authorization_url: Option<&Url>,
        token_url: Option<&Url>,
        refresh_url: Option<&Url>,
        scopes: &[(String, String)],
    ) -> Value {
        let mut map = Map::new();
        if let Some(url) = authorization_url {
            map.insert("authorizationUrl".into(), url.as_str().into());
        }
        if let Some(url) = token_url {
            map.insert("tokenUrl".into(), url.as_str().into());
        }
        if let Some(url) = refresh_url {
            map.insert("refreshUrl".into(), url.as_str().into());
        }
        map.insert(
            "scopes".into(),
            Value::Object(scopes.iter().map(|(k, v)| (k.clone(), v.clone().into())).collect()),
        );
        Value::Object(map)
    }

    impl Security {
        pub(crate) fn schema(&self) -> Value {
            let mut flows = json!({});
            if let Some(f) = &self.implicit {
                flows["implicit"] = flow(Some(&f.authorization_url), None, f.refresh_url.as_ref(), &f.scopes);
            }
            if let Some(f) = &self.password {
                flows["password"] = flow(None, Some(&f.token_url), f.refresh_url.as_ref(), &f.scopes);
            }
            if let Some(f) = &self.client_credentials {
                flows["clientCredentials"] = flow(None, Some(&f.token_url), f.refresh_url.as_ref(), &f.scopes);
            }
            if let Some(f) = &self.authorization_code {
                flows["authorizationCode"] = flow(
                    Some(&f.authorization_url),
                    Some(&f.token_url),
                    f.refresh_url.as_ref(),
                    &f.scopes,
                );
            }
            flows
        }
    }
}
