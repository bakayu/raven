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
    name = "list all agent tokens",
    skip(db),
    fields(include_revoked = include_revoked)
)]
pub async fn list_all_agent_tokens(
    db: &SqlitePool,
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
            ORDER BY created_at DESC
            "#,
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
            WHERE revoked_at IS NULL
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(db)
        .await
    }
    .map_err(|e| {
        error!(error = %e, "failed to list all agent tokens");
        e
    })?;

    debug!(count = rows.len(), "listed all agent tokens");
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
    name = "revoke agent token (admin)",
    skip(db),
    fields(token_id = %token_id)
)]
pub async fn revoke_agent_token_any(db: &SqlitePool, token_id: &str) -> AppResult<bool> {
    let res = sqlx::query!(
        r#"
        UPDATE agent_tokens
        SET revoked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE id = ?
          AND revoked_at IS NULL
        "#,
        token_id
    )
    .execute(db)
    .await
    .map_err(|e| {
        error!(error = %e, token_id = %token_id, "failed to revoke agent token (admin)");
        e
    })?;

    Ok(res.rows_affected() > 0)
}

#[tracing::instrument(
    name = "touch agent token last_used_at",
    skip(db),
    fields(token_id = %token_id)
)]
pub async fn touch_agent_token(db: &SqlitePool, token_id: &str) -> AppResult<()> {
    sqlx::query!(
        r#"
        UPDATE agent_tokens
        SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE id = ?
        "#,
        token_id
    )
    .execute(db)
    .await
    .map_err(|e| {
        error!(error = %e, token_id = %token_id, "failed to touch agent token");
        e
    })?;

    Ok(())
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

    async fn create_user(db: &sqlx::SqlitePool, username: &str) -> String {
        sqlx::query_scalar(
            r#"
            INSERT INTO users (username, password_hash, role)
            VALUES (?, ?, 'admin')
            RETURNING id
            "#,
        )
        .bind(username)
        .bind("hash")
        .fetch_one(db)
        .await
        .expect("insert user")
    }

    #[tokio::test]
    async fn create_validate_and_revoke_agent_token() {
        let (db, path) = setup_db("agent_tokens_create_validate_revoke").await;

        let owner_id = create_user(&db.write, "token-owner").await;

        let token_id = create_agent_token(&db.write, "api-token", "rvn_test_token", &owner_id)
            .await
            .expect("create agent token");

        let validated = validate_agent_token(&db.read, "rvn_test_token")
            .await
            .expect("validate token")
            .expect("token exists");
        assert_eq!(validated, token_id);

        let listed = list_agent_tokens(&db.read, &owner_id, false)
            .await
            .expect("list tokens");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, token_id);
        assert_eq!(listed[0].name, "api-token");
        assert_eq!(listed[0].created_by, owner_id);

        let revoked = revoke_agent_token(&db.write, &token_id, &owner_id)
            .await
            .expect("revoke token");
        assert!(revoked);

        let no_longer_valid = validate_agent_token(&db.read, "rvn_test_token")
            .await
            .expect("validate revoked token");
        assert!(no_longer_valid.is_none());

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
