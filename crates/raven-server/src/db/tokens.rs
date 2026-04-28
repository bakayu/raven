use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tracing::{error, warn};

use crate::error::AppResult;

/// SHA-256 hash of the raw token string, returned as a hex string.
pub fn hash_token(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

#[tracing::instrument(
    name="validating agent token",
    skip(db, raw_token),
    fields(token_hash = %hash_token(raw_token))
)]
pub async fn validate_agent_token(db: &SqlitePool, raw_token: &str) -> AppResult<Option<String>> {
    let hash = hash_token(raw_token);

    let row = sqlx::query!(
        r#"
        SELECT id as "id!"
        FROM agent_tokens
        WHERE token_hash = ?
          AND revoked_at IS NULL
        "#,
        hash
    )
    .fetch_optional(db)
    .await
    .map_err(|e| {
        error!(error = %e, "failed to validate agent token");
        e
    })?;

    if row.is_none() {
        warn!("agent token not found or revoked");
    }

    Ok(row.map(|r| r.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();

        std::env::temp_dir().join(format!("{}_{}_{}.db", name, std::process::id(), nanos))
    }

    async fn setup_db(name: &str) -> (Db, PathBuf) {
        let path = temp_db_path(name);
        let db = Db::connect(path.to_str().expect("utf8 path"))
            .await
            .expect("connect db");

        sqlx::migrate!("./migrations")
            .run(&db.write)
            .await
            .expect("run migrations");

        (db, path)
    }

    #[tokio::test]
    async fn hash_token_is_stable_and_not_plaintext() {
        let first = hash_token("rvn_test_token");
        let second = hash_token("rvn_test_token");

        assert_eq!(first, second);
        assert_ne!(first, "rvn_test_token");
        assert_eq!(first.len(), 64);
    }

    #[tokio::test]
    async fn validate_agent_token_finds_seeded_token() {
        let (db, path) = setup_db("validate_seeded").await;

        let expected_id: String =
            sqlx::query_scalar("SELECT id FROM agent_tokens WHERE name = 'dev-token' LIMIT 1")
                .fetch_one(&db.write)
                .await
                .expect("fetch token id");

        let found = validate_agent_token(&db.write, "rvn_test_token")
            .await
            .expect("validate token");

        assert_eq!(found.as_deref(), Some(expected_id.as_str()));

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn validate_agent_token_returns_none_for_unknown_token() {
        let (db, path) = setup_db("validate_unknown").await;

        let found = validate_agent_token(&db.write, "rvn_wrong_token")
            .await
            .expect("validate token");

        assert!(found.is_none());

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
