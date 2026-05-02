use sqlx::SqlitePool;

use crate::error::AppResult;

#[derive(Debug)]
pub struct UserIdentity {
    pub id: String,
    pub user_id: String,
    pub provider: String,
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: i64,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

pub async fn list_by_user(db: &SqlitePool, user_id: &str) -> AppResult<Vec<UserIdentity>> {
    let rows = sqlx::query_as!(
        UserIdentity,
        r#"
        SELECT
            id as "id!",
            user_id as "user_id!",
            provider as "provider!",
            issuer as "issuer!",
            subject as "subject!",
            email,
            email_verified,
            created_at as "created_at!",
            last_login_at
        FROM user_identities
        WHERE user_id = ?
        ORDER BY created_at DESC
        "#,
        user_id
    )
    .fetch_all(db)
    .await?;

    Ok(rows)
}

pub async fn delete_for_user(db: &SqlitePool, user_id: &str, identity_id: &str) -> AppResult<bool> {
    let res = sqlx::query!(
        r#"
        DELETE FROM user_identities
        WHERE id = ? AND user_id = ?
        "#,
        identity_id,
        user_id
    )
    .execute(db)
    .await?;

    Ok(res.rows_affected() > 0)
}

pub async fn upsert_identity(
    db: &SqlitePool,
    user_id: &str,
    provider: &str,
    issuer: &str,
    subject: &str,
    email: Option<&str>,
    email_verified: bool,
) -> AppResult<String> {
    let email_verified_value: i64 = if email_verified { 1 } else { 0 };

    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO user_identities (user_id, provider, issuer, subject, email, email_verified)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(issuer, subject)
        DO UPDATE SET
            user_id = excluded.user_id,
            provider = excluded.provider,
            email = excluded.email,
            email_verified = excluded.email_verified,
            last_login_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        RETURNING id as "id!"
        "#,
        user_id,
        provider,
        issuer,
        subject,
        email,
        email_verified_value,
    )
    .fetch_one(db)
    .await?;

    Ok(id)
}
