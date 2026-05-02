use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{delete, get, put},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api::middleware::RequestContext;
use crate::auth::{
    middleware::{RequireAdmin, RequireAuth},
    password,
};
use crate::db::{audit, identities, users};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users).post(create_user))
        .route("/me", get(get_me).put(update_me))
        .route("/me/password", put(change_password))
        .route("/me/identities", get(list_identities))
        .route("/me/identities/{id}", delete(delete_identity))
        .route("/{id}", get(get_user).put(update_user).delete(delete_user))
}

#[derive(Debug, Deserialize)]
struct CreateUserRequest {
    username: String,
    email: Option<String>,
    password: Option<String>,
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateUserRequest {
    username: Option<String>,
    email: Option<String>,
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

#[derive(Debug, Serialize)]
struct UserResponse {
    id: String,
    username: String,
    email: Option<String>,
    role: String,
    created_at: String,
    updated_at: String,
    last_login_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct IdentityResponse {
    id: String,
    provider: String,
    issuer: String,
    subject: String,
    email: Option<String>,
    email_verified: i64,
    created_at: String,
    last_login_at: Option<String>,
}

fn map_user(row: users::User) -> UserResponse {
    UserResponse {
        id: row.id,
        username: row.username,
        email: row.email,
        role: row.role,
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_login_at: row.last_login_at,
    }
}

fn map_user_record(row: users::UserRecord) -> UserResponse {
    UserResponse {
        id: row.id,
        username: row.username,
        email: row.email,
        role: row.role,
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_login_at: row.last_login_at,
    }
}

fn require_admin_or_self(claims: &crate::auth::jwt::Claims, user_id: &str) -> Result<(), AppError> {
    if claims.role == "admin" || claims.sub == user_id {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn list_users(
    State(state): State<AppState>,
    RequireAdmin(_claims): RequireAdmin,
) -> AppResult<Json<Vec<UserResponse>>> {
    let rows = users::list(&state.db.read).await?;
    Ok(Json(rows.into_iter().map(map_user_record).collect()))
}

async fn create_user(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    RequireAdmin(claims): RequireAdmin,
    Json(body): Json<CreateUserRequest>,
) -> AppResult<(hyper::StatusCode, Json<UserResponse>)> {
    let username = body.username.trim();
    if username.is_empty() {
        return Err(AppError::Validation("username is required".into()));
    }

    let role = body.role.as_deref().unwrap_or("member");
    if !matches!(role, "admin" | "member") {
        return Err(AppError::Validation("role must be admin or member".into()));
    }

    let password_hash = match body.password.as_deref() {
        Some(password) if !password.is_empty() => Some(password::hash(password)?),
        Some(_) => return Err(AppError::Validation("password cannot be empty".into())),
        None => None,
    };

    let user_id = users::create_with_optional_password(
        &state.db.write,
        username,
        body.email.as_deref(),
        password_hash.as_deref(),
        role,
    )
    .await?;

    let user = users::find_by_id(&state.db.read, &user_id)
        .await?
        .ok_or(AppError::UserNotFound)?;

    let _ = audit::insert(
        &state.db.write,
        audit::AuditEntry {
            actor_user_id: Some(&claims.sub),
            action: "user.create",
            entity_type: "user",
            entity_id: Some(&user_id),
            metadata: json!({"username": username, "role": role}),
            ip: ctx.client_ip.as_deref(),
            user_agent: ctx.user_agent.as_deref(),
            request_id: Some(&ctx.request_id),
        },
    )
    .await;

    Ok((hyper::StatusCode::CREATED, Json(map_user(user))))
}

async fn get_user(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Path(id): Path<String>,
) -> AppResult<Json<UserResponse>> {
    require_admin_or_self(&claims, &id)?;
    let user = users::find_by_id(&state.db.read, &id)
        .await?
        .ok_or(AppError::UserNotFound)?;
    Ok(Json(map_user(user)))
}

async fn update_user(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> AppResult<Json<UserResponse>> {
    require_admin_or_self(&claims, &id)?;

    if let Some(role) = body.role.as_deref() {
        if !matches!(role, "admin" | "member") {
            return Err(AppError::Validation("role must be admin or member".into()));
        }
        if claims.role != "admin" {
            return Err(AppError::Forbidden);
        }
        let _ = users::update_role(&state.db.write, &id, role).await?;
    }

    let _ = users::update_profile(
        &state.db.write,
        &id,
        body.username.as_deref(),
        body.email.as_deref(),
    )
    .await?;

    let user = users::find_by_id(&state.db.read, &id)
        .await?
        .ok_or(AppError::UserNotFound)?;

    Ok(Json(map_user(user)))
}

async fn delete_user(
    State(state): State<AppState>,
    RequireAdmin(claims): RequireAdmin,
    Path(id): Path<String>,
) -> AppResult<hyper::StatusCode> {
    if claims.sub == id {
        return Err(AppError::Forbidden);
    }

    if !users::delete(&state.db.write, &id).await? {
        return Err(AppError::UserNotFound);
    }

    Ok(hyper::StatusCode::NO_CONTENT)
}

async fn get_me(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
) -> AppResult<Json<UserResponse>> {
    let user = users::find_by_id(&state.db.read, &claims.sub)
        .await?
        .ok_or(AppError::UserNotFound)?;
    Ok(Json(map_user(user)))
}

async fn update_me(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Json(body): Json<UpdateUserRequest>,
) -> AppResult<Json<UserResponse>> {
    if let Some(role) = body.role.as_deref()
        && role != "member"
    {
        return Err(AppError::Forbidden);
    }

    let _ = users::update_profile(
        &state.db.write,
        &claims.sub,
        body.username.as_deref(),
        body.email.as_deref(),
    )
    .await?;
    let user = users::find_by_id(&state.db.read, &claims.sub)
        .await?
        .ok_or(AppError::UserNotFound)?;
    Ok(Json(map_user(user)))
}

async fn change_password(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Json(body): Json<ChangePasswordRequest>,
) -> AppResult<hyper::StatusCode> {
    let user = users::find_by_id(&state.db.read, &claims.sub)
        .await?
        .ok_or(AppError::UserNotFound)?;

    let stored = user
        .password_hash
        .as_deref()
        .ok_or(AppError::Unauthorized)?;
    if !password::verify(&body.current_password, stored)? {
        return Err(AppError::Unauthorized);
    }

    let new_hash = password::hash(&body.new_password)?;
    let _ = users::update_password_hash(&state.db.write, &claims.sub, &new_hash).await?;
    Ok(hyper::StatusCode::NO_CONTENT)
}

async fn list_identities(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
) -> AppResult<Json<Vec<IdentityResponse>>> {
    let rows = identities::list_by_user(&state.db.read, &claims.sub).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| IdentityResponse {
                id: row.id,
                provider: row.provider,
                issuer: row.issuer,
                subject: row.subject,
                email: row.email,
                email_verified: row.email_verified,
                created_at: row.created_at,
                last_login_at: row.last_login_at,
            })
            .collect(),
    ))
}

async fn delete_identity(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Path(id): Path<String>,
) -> AppResult<hyper::StatusCode> {
    if !identities::delete_for_user(&state.db.write, &claims.sub, &id).await? {
        return Err(AppError::UserNotFound);
    }
    Ok(hyper::StatusCode::NO_CONTENT)
}
