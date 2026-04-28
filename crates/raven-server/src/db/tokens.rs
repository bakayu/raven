use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tracing::{debug, error, warn};

use crate::error::AppResult;

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentToken {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

/// SHA-256 hash of the raw token string, returned as a hex string.
pub fn hash_token(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

#[tracing::instrument(
    name = "create agent token",
    skip(db, raw_token),
    fields(name = %name, created_by = %created_by)
)]
pub async fn create_agent_token(
    db: &SqlitePool,
    name: &str,
    raw_token: &str,
    created_by: &str,
) -> AppResult<String> {
    let token_hash = hash_token(raw_token);

    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO agent_tokens (name, token_hash, created_by)
        VALUES (?, ?, ?)
        RETURNING id as "id!"
        "#,
        name,
        token_hash,
        created_by
    )
    .fetch_one(db)
    .await
    .map_err(|e| {
        error!(error = %e, name = %name, created_by = %created_by, "failed to create agent token");
        e
    })?;

    debug!(token_id = %id, name = %name, created_by = %created_by, "agent token created");
    Ok(id)
}

#[tracing::instrument(
    name = "list agent tokens",
    skip(db),
    fields(created_by = %created_by, include_revoked = include_revoked)
)]
pub async fn list_agent_tokens(
    db: &SqlitePool,
    created_by: &str,
    include_revoked: bool,
) -> AppResult<Vec<AgentToken>> {
    let rows = if include_revoked {
        sqlx::query_as!(
            AgentToken,
            r#"
            SELECT
                id as "id!",
                name as "name!",
                created_by as "created_by!",
                created_at as "created_at!",
                last_used_at,
                revoked_at
            FROM agent_tokens
            WHERE created_by = ?
            ORDER BY created_at DESC
            "#,
            created_by
        )
        .fetch_all(db)
        .await
    } else {
        sqlx::query_as!(
            AgentToken,
            r#"
            SELECT
                id as "id!",
                name as "name!",
                created_by as "created_by!",
                created_at as "created_at!",
                last_used_at,
                revoked_at
            FROM agent_tokens
            WHERE created_by = ?
              AND revoked_at IS NULL
            ORDER BY created_at DESC
            "#,
            created_by
        )
        .fetch_all(db)
        .await
    }
    .map_err(|e| {
        error!(error = %e, created_by = %created_by, "failed to list agent tokens");
        e
    })?;

    debug!(count = rows.len(), created_by = %created_by, "listed agent tokens");
    Ok(rows)
}

#[tracing::instrument(
    name = "revoke agent token",
    skip(db),
    fields(token_id = %token_id, created_by = %created_by)
)]
pub async fn revoke_agent_token(
    db: &SqlitePool,
    token_id: &str,
    created_by: &str,
) -> AppResult<bool> {
    let res = sqlx::query!(
        r#"
        UPDATE agent_tokens
        SET revoked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE id = ?
          AND created_by = ?
          AND revoked_at IS NULL
        "#,
        token_id,
        created_by
    )
    .execute(db)
    .await
    .map_err(|e| {
        error!(error = %e, token_id = %token_id, created_by = %created_by, "failed to revoke agent token");
        e
    })?;

    let revoked = res.rows_affected() > 0;
    if !revoked {
        warn!(token_id = %token_id, created_by = %created_by, "no active token revoked");
    } else {
        debug!(token_id = %token_id, created_by = %created_by, "agent token revoked");
    }

    Ok(revoked)
}

#[tracing::instrument(
    name = "validating agent token",
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
