//! The router state, available to every handler as `ctx.state`.

use std::sync::Arc;

use crate::config::Config;
use crate::store::TaskStore;

/// Cheap to clone: everything is behind an `Arc`.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub tasks: Arc<TaskStore>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            tasks: Arc::new(TaskStore::default()),
        }
    }
}
