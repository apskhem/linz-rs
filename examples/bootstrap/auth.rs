//! Authentication: a bearer token security scheme and the user it resolves to.

use http::{
    Extensions,
    StatusCode,
};
use linz_rs::security::AuthenticationScheme;
use linz_rs::{
    ApiError,
    Credentials,
    Security,
    SecurityType,
};

/// Allows creating, changing and deleting tasks.
pub const TASKS_WRITE: &str = "tasks:write";

/// The authenticated user, inserted into the request extensions by the security handler.
/// Handlers read it with `ctx.extension::<CurrentUser>()?`.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub name: String,
    pub scopes: Vec<String>,
}

/// A bearer token scheme that accepts a single static token.
///
/// Replace the token comparison with real verification (a JWT library, a session lookup, ...).
/// Security handlers do not receive the router state, so capture what they need here.
pub fn bearer(api_token: String) -> Security {
    Security::new(
        "bearerAuth",
        SecurityType::Http {
            scheme: AuthenticationScheme::Bearer,
            bearer_format: None,
        },
        move |credentials: Credentials| {
            let api_token = api_token.clone();
            async move {
                if credentials.token.as_deref() != Some(api_token.as_str()) {
                    return Err(ApiError::detail(StatusCode::UNAUTHORIZED, "Invalid token"));
                }

                let user = CurrentUser {
                    name: "admin".to_string(),
                    scopes: vec![TASKS_WRITE.to_string()],
                };
                if let Some(missing) = credentials.scopes.iter().find(|scope| !user.scopes.contains(scope)) {
                    return Err(ApiError::detail(
                        StatusCode::FORBIDDEN,
                        format!("Missing scope {missing}"),
                    ));
                }

                let mut extensions = Extensions::new();
                extensions.insert(user);
                Ok(extensions)
            }
        },
    )
    .describe("Send `Authorization: Bearer <API_TOKEN>`")
}
