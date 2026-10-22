//! A liveness probe for load balancers and orchestrators.

use linz_rs::*;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Serialize, JsonSchema)]
pub struct Health {
    /// Always `ok` when the service responds.
    status: &'static str,
    /// The version of the service.
    version: &'static str,
}

/// Generic over the state, because it does not read it.
pub fn routes<S: Clone + Send + Sync + 'static>() -> Api<S> {
    Api::new()
        .get(
            "/health",
            Endpoint::new("getHealth")
                .summary("Check that the service is up")
                .returns(200, JsonBody::new::<Health>()),
            |_, _| {
                async move {
                    Ok(Health {
                        status: "ok",
                        version: env!("CARGO_PKG_VERSION"),
                    })
                }
            },
        )
        .config(GroupConfig::new().tag("System"))
}
