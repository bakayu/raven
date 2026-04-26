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
