use sqlx::SqlitePool;
use tracing::{debug, error, warn};

use raven_proto::proto::RegisterRequest;

use crate::{AppError, AppResult};

/// Insert new agent on register, update if token_id and hostname exist
/// in the agents table
#[tracing::instrument(
    name="Upserting agent to the database",
    skip(db, request),
    fields(token_id = %token_id, hostname = %request.hostname)
)]
pub async fn upsert_agent(
    db: &SqlitePool,
    request: &RegisterRequest,
    token_id: &str,
    ip: Option<&str>,
) -> AppResult<()> {
    let log_files_json = serde_json::to_string(&request.log_files).map_err(|e| {
        error!(
            error = %e,
            hostname = %request.hostname,
            agent_version = %request.agent_version,
            "failed to serialize agent log_files"
        );
        e
    })?;

    let res = sqlx::query!(
        r#"
        INSERT INTO agents (token_id, hostname, ip, os, agent_version, log_files)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(token_id, hostname)
        DO UPDATE SET
            ip = excluded.ip,
            os = excluded.os,
            agent_version = excluded.agent_version,
            log_files = excluded.log_files,
            last_seen_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        "#,
        token_id,
        request.hostname,
        ip,
        request.os,
        request.agent_version,
        log_files_json
    )
    .execute(db)
    .await?;

    debug!(
        token_id = %token_id,
        hostname = %request.hostname,
        rows_affected = res.rows_affected(),
        "agent upsert completed"
    );

    Ok(())
}

/// update "last_seen_at" for an agent
#[tracing::instrument(
    name="updating last seen of agent",
    skip(db),
    fields(token_id = %token_id, hostname = %hostname)
)]
pub async fn update_last_seen(db: &SqlitePool, token_id: &str, hostname: &str) -> AppResult<()> {
    let res = sqlx::query!(
        r#"
        UPDATE agents SET last_seen_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE token_id = ? AND hostname = ?
        "#,
        token_id,
        hostname
    )
    .execute(db)
    .await
    .map_err(|e| {
        error!(
            error = %e,
            token_id = %token_id,
            hostname = %hostname,
            "failed to update last_seen_at"
        );
        e
    })?;

    if res.rows_affected() == 0 {
        warn!(
            token_id = %token_id,
            hostname = %hostname,
            "no agent row matched for last_seen_at update"
        );
        return Err(AppError::AgentNotFound);
    }

    debug!(
        token_id = %token_id,
        hostname = %hostname,
        rows_affected = res.rows_affected(),
        "last_seen_at updated"
    );

    Ok(())
}
