pub mod agents;
pub mod audit;
pub mod identities;
pub mod sessions;
pub mod tokens;
pub mod users;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use std::str::FromStr;

#[derive(Debug)]
pub struct Db {
    pub write: SqlitePool,
    pub read: SqlitePool,
}

impl Db {
    pub async fn connect(path: &str) -> anyhow::Result<Self> {
        // ensure the parent directory exists before SQLite tries to create the file
        let db_path = std::path::Path::new(path);
        if let Some(parent) = db_path.parent()
            && !parent.exists()
        {
            anyhow::bail!("database directory does not exist: {}", parent.display());
        }

        let url = format!("sqlite://{path}");
        let opts = SqliteConnectOptions::from_str(&url)?
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .create_if_missing(true);

        let write = sqlx::pool::PoolOptions::<sqlx::Sqlite>::new()
            .max_connections(1)
            .connect_with(opts.clone())
            .await?;

        let read = sqlx::pool::PoolOptions::<sqlx::Sqlite>::new()
            .max_connections(8)
            .connect_with(opts.read_only(true))
            .await?;

        Ok(Self { write, read })
    }
}
