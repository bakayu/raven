use crate::configuration::RavenConfig;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<RavenConfig>,
    pub dev_token: String,
}

impl AppState {
    pub fn new(config: RavenConfig, dev_token: String) -> Self {
        Self {
            config: Arc::new(config),
            dev_token,
        }
    }
}
