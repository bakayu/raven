use crate::{Db, configuration::RavenConfig};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<RavenConfig>,
    pub db: Arc<Db>,
}

impl AppState {
    pub fn new(config: RavenConfig, db: Db) -> Self {
        Self {
            config: Arc::new(config),
            db: Arc::new(db),
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

        AppState {
            config: std::sync::Arc::new(RavenConfig::for_test()),
            db: std::sync::Arc::new(db),
        }
    }
}
