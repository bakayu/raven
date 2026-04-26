use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::error::AppResult;

/// SHA-256 hash of the raw token string, returned as a hex string.
pub fn hash_token(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

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
    .await?;

    Ok(row.map(|r| r.id))
}

/// Seeds a hardcoded dev token for local development.
/// Only inserts if no tokens exist yet, safe to call on every startup.
/// TODO: Remove this once the setup flow (user creation -> token generation) is built.
pub async fn seed_dev_token(db: &SqlitePool, raw_token: &str) -> anyhow::Result<()> {
    let count: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM agent_tokens")
        .fetch_one(db)
        .await?;

    if count > 0 {
        return Ok(());
    }

    let hash = hash_token(raw_token);
    let name = "dev-token";

    // No created_by user exists yet, so we use a placeholder ID.
    // The FK references users(id) ON DELETE CASCADE - this will need
    // a real user_id once the setup flow is built.
    // For now: insert a placeholder system user first.
    sqlx::query!(
        "INSERT OR IGNORE INTO users (id, username, role) VALUES ('system', 'system', 'admin')"
    )
    .execute(db)
    .await?;

    sqlx::query!(
        r#"
        INSERT INTO agent_tokens (name, token_hash, created_by)
        VALUES (?, ?, 'system')
        "#,
        name,
        hash,
    )
    .execute(db)
    .await?;

    tracing::info!(
        token = raw_token,
        "dev token seeded — remove before production"
    );
    Ok(())
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
    async fn seed_dev_token_inserts_only_once() {
        let (db, path) = setup_db("seed_once").await;

        seed_dev_token(&db.write, "rvn_test_token")
            .await
            .expect("seed dev token");
        seed_dev_token(&db.write, "rvn_other_token")
            .await
            .expect("second seed should be ignored");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_tokens")
            .fetch_one(&db.write)
            .await
            .expect("count rows");

        assert_eq!(count, 1);

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn validate_agent_token_finds_seeded_token() {
        let (db, path) = setup_db("validate_seeded").await;

        seed_dev_token(&db.write, "rvn_test_token")
            .await
            .expect("seed dev token");

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

        seed_dev_token(&db.write, "rvn_test_token")
            .await
            .expect("seed dev token");

        let found = validate_agent_token(&db.write, "rvn_wrong_token")
            .await
            .expect("validate token");

        assert!(found.is_none());

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
