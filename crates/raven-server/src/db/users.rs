use sqlx::SqlitePool;

use crate::error::AppResult;

#[derive(Debug)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub role: String,
    pub failed_login_attempts: i64,
    pub auth_locked_until: Option<String>,
}

pub async fn count(db: &SqlitePool) -> AppResult<i64> {
    Ok(sqlx::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(db)
        .await?)
}

pub async fn create(
    db: &SqlitePool,
    username: &str,
    password_hash: &str,
    role: &str,
) -> AppResult<String> {
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO users (username, password_hash, role)
        VALUES (?, ?, ?)
        RETURNING id as "id!"
        "#,
        username,
        password_hash,
        role,
    )
    .fetch_one(db)
    .await?;

    Ok(id)
}

pub async fn find_by_username(db: &SqlitePool, username: &str) -> AppResult<Option<User>> {
    let row = sqlx::query_as!(
        User,
        r#"
        SELECT id as "id!", username, email, password_hash, role,
               failed_login_attempts, auth_locked_until
        FROM users
        WHERE username = ?
        "#,
        username
    )
    .fetch_optional(db)
    .await?;

    Ok(row)
}

pub async fn update_failed_attempts(
    db: &SqlitePool,
    user_id: &str,
    attempts: i64,
    locked_until: Option<&str>,
) -> AppResult<()> {
    sqlx::query!(
        r#"
        UPDATE users
        SET failed_login_attempts = ?,
            auth_locked_until = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE id = ?
        "#,
        attempts,
        locked_until,
        user_id,
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn clear_failed_attempts(db: &SqlitePool, user_id: &str) -> AppResult<()> {
    sqlx::query!(
        r#"
        UPDATE users
        SET failed_login_attempts = 0,
            auth_locked_until = NULL,
            last_login_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE id = ?
        "#,
        user_id,
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

    #[tokio::test]
    async fn count_starts_zero() {
        let (db, path) = setup_db("users_count_zero").await;

        let n = count(&db.read).await.expect("count users");
        assert_eq!(n, 0);

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn create_and_find_by_username() {
        let (db, path) = setup_db("users_create_find").await;

        let user_id = create(&db.write, "alice", "hash123", "admin")
            .await
            .expect("create user");

        let user = find_by_username(&db.read, "alice")
            .await
            .expect("find user")
            .expect("user exists");

        assert_eq!(user.id, user_id);
        assert_eq!(user.username, "alice");
        assert_eq!(user.password_hash.as_deref(), Some("hash123"));
        assert_eq!(user.role, "admin");
        assert_eq!(user.failed_login_attempts, 0);
        assert!(user.auth_locked_until.is_none());

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn update_failed_attempts_sets_lock_fields() {
        let (db, path) = setup_db("users_failed_attempts").await;

        let user_id = create(&db.write, "bob", "hash123", "member")
            .await
            .expect("create user");

        update_failed_attempts(&db.write, &user_id, 3, Some("2099-01-01T00:00:00Z"))
            .await
            .expect("update failed attempts");

        let user = find_by_username(&db.read, "bob")
            .await
            .expect("find user")
            .expect("user exists");

        assert_eq!(user.failed_login_attempts, 3);
        assert_eq!(
            user.auth_locked_until.as_deref(),
            Some("2099-01-01T00:00:00Z")
        );

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn clear_failed_attempts_resets_lock_state() {
        let (db, path) = setup_db("users_clear_failed_attempts").await;

        let user_id = create(&db.write, "carol", "hash123", "member")
            .await
            .expect("create user");

        update_failed_attempts(&db.write, &user_id, 5, Some("2099-01-01T00:00:00Z"))
            .await
            .expect("set lock state");

        clear_failed_attempts(&db.write, &user_id)
            .await
            .expect("clear failed attempts");

        let user = find_by_username(&db.read, "carol")
            .await
            .expect("find user")
            .expect("user exists");

        assert_eq!(user.failed_login_attempts, 0);
        assert!(user.auth_locked_until.is_none());

        let last_login: Option<String> =
            sqlx::query_scalar("SELECT last_login_at FROM users WHERE id = ?")
                .bind(&user_id)
                .fetch_one(&db.read)
                .await
                .expect("fetch last_login_at");
        assert!(last_login.is_some());

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
