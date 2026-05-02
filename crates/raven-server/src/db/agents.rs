use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use tracing::{debug, error, warn};

use raven_proto::proto::RegisterRequest;

use crate::{AppError, AppResult};

#[derive(Debug, Serialize)]
pub struct AgentRow {
    pub id: String,
    pub token_id: String,
    pub hostname: String,
    pub ip: Option<String>,
    pub os: String,
    pub agent_version: String,
    pub log_files: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
}

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

#[tracing::instrument(name = "listing all agents", skip(db))]
pub async fn list_agents(db: &SqlitePool) -> AppResult<Vec<AgentRow>> {
    let rows = sqlx::query_as!(
        AgentRow,
        r#"
        SELECT
            id as "id!",
            token_id,
            hostname,
            ip,
            os as "os!",
            agent_version as "agent_version!",
            log_files,
            first_seen_at,
            last_seen_at
        FROM agents
        ORDER BY first_seen_at DESC
        "#
    )
    .fetch_all(db)
    .await
    .map_err(|e| {
        error!(error = %e, "failed to list agents");
        e
    })?;

    Ok(rows)
}

/// List agents filtered by token ownership (for non-admin users).
#[tracing::instrument(
    name = "listing agents for user",
    skip(db),
    fields(user_id = %user_id)
)]
pub async fn list_agents_for_user(db: &SqlitePool, user_id: &str) -> AppResult<Vec<AgentRow>> {
    let rows = sqlx::query_as!(
        AgentRow,
        r#"
        SELECT
            a.id as "id!",
            a.token_id,
            a.hostname,
            a.ip,
            a.os as "os!",
            a.agent_version as "agent_version!",
            a.log_files,
            a.first_seen_at,
            a.last_seen_at
        FROM agents a
        INNER JOIN agent_tokens t ON a.token_id = t.id
        WHERE t.created_by = ?
        ORDER BY a.first_seen_at DESC
        "#,
        user_id
    )
    .fetch_all(db)
    .await
    .map_err(|e| {
        error!(error = %e, user_id = %user_id, "failed to list agents for user");
        e
    })?;

    debug!(count = rows.len(), user_id = %user_id, "listed agents for user");
    Ok(rows)
}

/// Delete an agent by id, scoped to the owner of the associated token.
#[tracing::instrument(
    name = "deleting agent",
    skip(db),
    fields(agent_id = %agent_id, user_id = %user_id)
)]
pub async fn delete_agent(db: &SqlitePool, agent_id: &str, user_id: &str) -> AppResult<bool> {
    let res = sqlx::query!(
        r#"
        DELETE FROM agents
        WHERE id = ?
          AND token_id IN (SELECT id FROM agent_tokens WHERE created_by = ?)
        "#,
        agent_id,
        user_id
    )
    .execute(db)
    .await
    .map_err(|e| {
        error!(error = %e, agent_id = %agent_id, user_id = %user_id, "failed to delete agent");
        e
    })?;

    let deleted = res.rows_affected() > 0;
    if deleted {
        debug!(agent_id = %agent_id, user_id = %user_id, "agent deleted");
    } else {
        warn!(agent_id = %agent_id, user_id = %user_id, "no agent deleted (not found or not owned)");
    }
    Ok(deleted)
}

/// Delete an agent by id without ownership check (admin).
#[tracing::instrument(
    name = "deleting agent (admin)",
    skip(db),
    fields(agent_id = %agent_id)
)]
pub async fn delete_agent_any(db: &SqlitePool, agent_id: &str) -> AppResult<bool> {
    let res = sqlx::query!(
        r#"
        DELETE FROM agents WHERE id = ?
        "#,
        agent_id
    )
    .execute(db)
    .await
    .map_err(|e| {
        error!(error = %e, agent_id = %agent_id, "failed to delete agent (admin)");
        e
    })?;

    Ok(res.rows_affected() > 0)
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

/// load last_seen_at of all available agents from the dashboard
#[tracing::instrument(name = "loading agents from database for DashMap", skip(db))]
pub async fn load_all_agents(db: &SqlitePool) -> AppResult<Vec<(String, String, DateTime<Utc>)>> {
    let rows = sqlx::query!(
        r#"
        SELECT token_id, hostname, last_seen_at
        FROM agents
        "#
    )
    .fetch_all(db)
    .await?;

    debug!(
        rows_loaded = rows.len(),
        "loaded agent data from dashboard to populate dashmap"
    );

    rows.into_iter()
        .map(|r| {
            let last_seen = chrono::DateTime::parse_from_rfc3339(&r.last_seen_at)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| AppError::Internal(format!("invalid last_seen_at: {e}")))?;
            Ok((r.token_id, r.hostname, last_seen))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use crate::db::{tokens::create_agent_token, tokens::validate_agent_token, users};
    use raven_proto::proto::RegisterRequest;
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

    async fn seed_agent_token(db: &Db) -> String {
        let owner_id = users::create(&db.write, "agent-owner", "hash123", "admin")
            .await
            .expect("create owner user");

        create_agent_token(&db.write, "test-agent", "rvn_test_token", &owner_id)
            .await
            .expect("create agent token")
    }

    fn sample_register(hostname: &str) -> RegisterRequest {
        RegisterRequest {
            agent_id: "agent-1".into(),
            hostname: hostname.into(),
            os: "linux".into(),
            agent_version: "0.1.0".into(),
            log_files: vec!["/var/log/app.log".into()],
        }
    }

    #[tokio::test]
    async fn load_all_agents_returns_inserted_agent() {
        let (db, path) = setup_db("load_all_agents").await;

        let token_id = seed_agent_token(&db).await;

        let validated = validate_agent_token(&db.read, "rvn_test_token")
            .await
            .expect("validate token")
            .expect("token exists");

        assert_eq!(validated, token_id);

        let req = sample_register("host-a");
        upsert_agent(&db.write, &req, token_id.as_str(), Some("10.0.0.1"))
            .await
            .expect("upsert agent");

        let rows = load_all_agents(&db.read).await.expect("load agents");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, token_id);
        assert_eq!(rows[0].1, "host-a");
        // basic sanity: parsed timestamp should be in UTC and represent a valid point in time
        assert!(rows[0].2.timestamp() > 0);

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn update_last_seen_returns_not_found_for_missing_agent() {
        let (db, path) = setup_db("update_last_seen_missing").await;

        let err = update_last_seen(&db.write, "missing-token-id", "missing-host")
            .await
            .expect_err("expected not found");

        match err {
            crate::AppError::AgentNotFound => {}
            other => panic!("unexpected error: {other}"),
        }

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn update_last_seen_changes_timestamp_for_existing_agent() {
        let (db, path) = setup_db("update_last_seen_ok").await;

        let token_id = seed_agent_token(&db).await;

        let validated = validate_agent_token(&db.read, "rvn_test_token")
            .await
            .expect("validate token")
            .expect("token exists");

        assert_eq!(validated, token_id);

        let req = sample_register("host-b");
        upsert_agent(&db.write, &req, token_id.as_str(), Some("10.0.0.2"))
            .await
            .expect("upsert agent");

        let old_value = "2000-01-01T00:00:00Z";
        sqlx::query("UPDATE agents SET last_seen_at = ? WHERE token_id = ? AND hostname = ?")
            .bind(old_value)
            .bind(&token_id)
            .bind("host-b")
            .execute(&db.write)
            .await
            .expect("force old last_seen_at");

        update_last_seen(&db.write, token_id.as_str(), "host-b")
            .await
            .expect("update last_seen");

        let updated: String = sqlx::query_scalar(
            "SELECT last_seen_at FROM agents WHERE token_id = ? AND hostname = ?",
        )
        .bind(&token_id)
        .bind("host-b")
        .fetch_one(&db.read)
        .await
        .expect("fetch updated timestamp");

        assert_ne!(updated, old_value);

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
