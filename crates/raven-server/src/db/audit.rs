use serde_json::Value;
use sqlx::SqlitePool;
use tracing::error;

use crate::error::AppResult;

pub struct AuditEntry<'a> {
    pub actor_user_id: Option<&'a str>,
    pub action: &'a str,
    pub entity_type: &'a str,
    pub entity_id: Option<&'a str>,
    pub metadata: Value,
    pub ip: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub request_id: Option<&'a str>,
}

pub async fn insert(db: &SqlitePool, entry: AuditEntry<'_>) -> AppResult<()> {
    let metadata = entry.metadata.to_string();

    sqlx::query!(
        r#"
        INSERT INTO audit_log (
            actor_user_id,
            action,
            entity_type,
            entity_id,
            metadata,
            ip,
            user_agent,
            request_id
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        entry.actor_user_id,
        entry.action,
        entry.entity_type,
        entry.entity_id,
        metadata,
        entry.ip,
        entry.user_agent,
        entry.request_id,
    )
    .execute(db)
    .await
    .map_err(|e| {
        error!(error = %e, action = %entry.action, entity_type = %entry.entity_type, "failed to insert audit log");
        e
    })?;

    Ok(())
}
