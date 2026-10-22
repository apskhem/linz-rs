//! The route tree. Each resource lives in its own module and exposes a `routes` function.

mod attachments;
mod health;
mod tasks;

use linz_rs::{
    Api,
    Security,
};

use crate::state::AppState;

/// Every endpoint of the service: `/health`, and the resources under `/v1`.
pub fn api(auth: &Security) -> Api<AppState> {
    Api::<AppState>::new().merge(health::routes()).nest(
        "/v1",
        Api::new().merge(tasks::routes(auth)).merge(attachments::routes(auth)),
    )
}
