use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;

use crate::{Db, configuration::RavenConfig};

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<RavenConfig>,
    pub db: Arc<Db>,
    pub agents: Arc<DashMap<String, AgentState>>,
}

#[derive(Debug, Clone)]
pub struct AgentState {
    pub hostname: String,
    pub agent_id: String,
    pub last_heartbeat: DateTime<Utc>,
}

impl AppState {
    pub fn new(config: RavenConfig, db: Db) -> Self {
        Self {
            config: Arc::new(config),
            db: Arc::new(db),
            agents: Arc::new(DashMap::new()),
        }
    }
}

#[cfg(test)]
impl AppState {
    pub async fn for_test() -> Self {
        let db_path = std::env::temp_dir().join(format!(
            "raven_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let db = Db::connect(db_path.to_str().expect("utf8 path"))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&db.write).await.unwrap();

        crate::db::tokens::seed_dev_token(&db.write, "rvn_test_token")
            .await
            .unwrap();

        let state = AppState {
            config: Arc::new(RavenConfig::for_test()),
            db: Arc::new(db),
            agents: Arc::new(DashMap::new()),
        };

        let known_agents = crate::db::agents::load_all_agents(&state.db.read)
            .await
            .expect("load known agents");
        for (token_id, hostname, last_seen) in known_agents {
            state.agents.insert(
                token_id.clone(),
                AgentState {
                    agent_id: token_id,
                    hostname,
                    last_heartbeat: last_seen,
                },
            );
        }

        state
    }
}
