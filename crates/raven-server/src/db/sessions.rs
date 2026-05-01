use sqlx::SqlitePool;

use crate::error::AppResult;

pub struct RefreshToken {
    pub id: String,
    pub user_id: String,
    pub token_hash: String,
    pub expires_at: String,
    pub revoked_at: Option<String>,
}

pub async fn insert_refresh_token(
    db: &SqlitePool,
    user_id: &str,
    token_hash: &str,
    expires_at: &str,
    rotated_from_token_id: Option<&str>,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> AppResult<String> {
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO refresh_tokens (
            user_id,
            token_hash,
            expires_at,
            rotated_from_token_id,
            created_by_ip,
            created_by_user_agent
        )
        VALUES (?, ?, ?, ?, ?, ?)
        RETURNING id as "id!"
        "#,
        user_id,
        token_hash,
        expires_at,
        rotated_from_token_id,
        ip,
        user_agent,
    )
    .fetch_one(db)
    .await?;

    Ok(id)
}

pub async fn find_by_hash(db: &SqlitePool, token_hash: &str) -> AppResult<Option<RefreshToken>> {
    let row = sqlx::query_as!(
        RefreshToken,
        r#"
        SELECT id as "id!", user_id, token_hash, expires_at, revoked_at
        FROM refresh_tokens
        WHERE token_hash = ?
        "#,
        token_hash
    )
    .fetch_optional(db)
    .await?;

    Ok(row)
}

pub async fn revoke_refresh_token(db: &SqlitePool, token_id: &str, reason: &str) -> AppResult<()> {
    sqlx::query!(
        r#"
        UPDATE refresh_tokens
        SET revoked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            revoked_reason = ?
        WHERE id = ?
        "#,
        reason,
        token_id,
    )
    .execute(db)
    .await?;

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
    async fn insert_and_find_refresh_token() {
        let (db, path) = setup_db("sessions_insert_find").await;

        let user_id = create_user(&db.write, "sess-user-1").await;

        let token_id = insert_refresh_token(
            &db.write,
            &user_id,
            "token_hash_1",
            "2099-01-01T00:00:00Z",
            None,
            Some("127.0.0.1"),
            Some("test-agent"),
        )
        .await
        .expect("insert refresh token");

        let row = find_by_hash(&db.read, "token_hash_1")
            .await
            .expect("find token")
            .expect("token exists");

        assert_eq!(row.id, token_id);
        assert_eq!(row.user_id, user_id);
        assert_eq!(row.token_hash, "token_hash_1");
        assert_eq!(row.expires_at, "2099-01-01T00:00:00Z");
        assert!(row.revoked_at.is_none());

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn revoke_refresh_token_sets_revoked_at_and_reason() {
        let (db, path) = setup_db("sessions_revoke").await;

        let user_id = create_user(&db.write, "sess-user-2").await;

        let token_id = insert_refresh_token(
            &db.write,
            &user_id,
            "token_hash_2",
            "2099-01-01T00:00:00Z",
            None,
            None,
            None,
        )
        .await
        .expect("insert refresh token");

        revoke_refresh_token(&db.write, &token_id, "rotation")
            .await
            .expect("revoke refresh token");

        let row = find_by_hash(&db.read, "token_hash_2")
            .await
            .expect("find token")
            .expect("token exists");
        assert!(row.revoked_at.is_some());

        let reason: Option<String> =
            sqlx::query_scalar("SELECT revoked_reason FROM refresh_tokens WHERE id = ?")
                .bind(&token_id)
                .fetch_one(&db.read)
                .await
                .expect("fetch revoked reason");
        assert_eq!(reason.as_deref(), Some("rotation"));

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
