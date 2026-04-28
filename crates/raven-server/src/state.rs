use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::broadcast;
use tracing::info;

use raven_proto::proto::LogBatch;

use crate::{
    ClickHouseClient, Db, VictoriaMetricsClient, configuration::RavenConfig,
    db::agents::load_all_agents,
};

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<RavenConfig>,
    pub db: Arc<Db>,
    pub agents: Arc<DashMap<String, AgentState>>,
    pub vm_client: Arc<VictoriaMetricsClient>,
    pub ch_client: Arc<ClickHouseClient>,
    pub log_tx: broadcast::Sender<LogBatch>,
}

#[derive(Debug, Clone)]
pub struct AgentState {
    pub hostname: String,
    pub agent_id: String,
    pub last_heartbeat: DateTime<Utc>,
}

impl AppState {
    pub async fn new(config: RavenConfig) -> anyhow::Result<Self> {
        let db = Db::connect(&config.database.sqlite_path).await?;
        sqlx::migrate!("./migrations").run(&db.write).await?;

        let vm = VictoriaMetricsClient::new(&config.database.victoria_metrics_url);
        let ch = ClickHouseClient::new(&config.database.clickhouse_url);

        let state = Self {
            config: Arc::new(config),
            db: Arc::new(db),
            agents: Arc::new(DashMap::new()),
            vm_client: Arc::new(vm),
            ch_client: Arc::new(ch),
            log_tx: broadcast::channel(1024).0,
        };

        state.ch_client.ensure_schema().await?;
        info!("clickhouse schema ready");

        let known_agents = load_all_agents(&state.db.read).await?;
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

        info!(agents = state.agents.len(), "loaded agents from database");

        Ok(state)
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

        let mut config = RavenConfig::for_test();
        config.database.sqlite_path = db_path.to_str().expect("utf8 path").to_string();

        let db = Db::connect(&config.database.sqlite_path).await.unwrap();
        sqlx::migrate!("./migrations").run(&db.write).await.unwrap();

        let vm_client = VictoriaMetricsClient::new(&config.database.victoria_metrics_url);
        let ch_client = ClickHouseClient::new(&config.database.clickhouse_url);

        let state = AppState {
            config: Arc::new(config),
            db: Arc::new(db),
            agents: Arc::new(DashMap::new()),
            vm_client: Arc::new(vm_client),
            ch_client: Arc::new(ch_client),
            log_tx: broadcast::channel(1024).0,
        };

        state.ch_client.ensure_schema().await.unwrap();

        let known_agents = load_all_agents(&state.db.read)
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
