use axum::http::HeaderValue;
use axum::http::header::{COOKIE, HeaderMap, SET_COOKIE};
use axum::{Json, Router, extract::State, routing::post};
use chrono::{Duration, SecondsFormat, Utc};
use hyper::StatusCode;
use rand::prelude::*;
use rand_distr::Alphanumeric;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::auth::{jwt, password};
use crate::db::{sessions, users};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tokens;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/setup", post(setup))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
}

#[derive(Debug, Deserialize)]
struct SetupRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct SetupResponse {
    message: String,
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    access_token: String,
}

// Extract refresh token helper from the cookie header
fn extract_refresh_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookie_str| {
            cookie_str
                .split(';')
                .map(|s| s.trim())
                .find(|s| s.starts_with("refresh_token="))
                .map(|s| s["refresh_token=".len()..].to_string())
        })
}

async fn setup(
    State(state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> AppResult<(StatusCode, Json<SetupResponse>)> {
    let username = body.username.trim();
    if username.is_empty() {
        return Err(AppError::Validation("username is required".into()));
    }
    if body.password.len() < 8 {
        return Err(AppError::Validation(
            "password must be at least 8 characters".into(),
        ));
    }

    let count = users::count(&state.db.read).await?;
    if count > 0 {
        return Err(AppError::SetupAlreadyDone);
    }

    let hash = password::hash(&body.password)?;
    let user_id = users::create(&state.db.write, username, &hash, "admin").await?;

    info!(username = %username, user_id = %user_id, "admin account created");

    Ok((
        StatusCode::CREATED,
        Json(SetupResponse {
            message: "admin account created".into(),
            user_id,
        }),
    ))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> AppResult<(HeaderMap, Json<LoginResponse>)> {
    let username = body.username.trim();
    if username.is_empty() || body.password.is_empty() {
        return Err(AppError::Unauthorized);
    }

    let user = users::find_by_username(&state.db.read, username)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let stored_hash = user
        .password_hash
        .as_deref()
        .ok_or(AppError::Unauthorized)?;
    if !password::verify(&body.password, stored_hash)? {
        return Err(AppError::Unauthorized);
    }

    let auth_cfg = &state.config.auth;
    let access_ttl_minutes = i64::try_from(auth_cfg.access_token_ttl_minutes)
        .map_err(|_| AppError::Internal("access_token_ttl_minutes is too large".into()))?;

    let access_token = jwt::issue(
        &user.id,
        &user.username,
        &user.role,
        &auth_cfg.jwt_issuer,
        &auth_cfg.jwt_audience,
        access_ttl_minutes,
        auth_cfg.jwt_signing_key.expose_secret(),
    )?;

    let refresh_token: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();

    let refresh_token_hash = tokens::hash_token(&refresh_token);

    let refresh_ttl_days_i64 = i64::try_from(auth_cfg.refresh_token_ttl_days)
        .map_err(|_| AppError::Internal("refresh_token_ttl_days is too large".into()))?;
    let refresh_max_age_seconds = auth_cfg
        .refresh_token_ttl_days
        .checked_mul(24 * 60 * 60)
        .ok_or_else(|| AppError::Internal("refresh token max-age overflow".into()))?;

    let expires_at = (Utc::now() + Duration::days(refresh_ttl_days_i64))
        .to_rfc3339_opts(SecondsFormat::Secs, true);

    sessions::insert_refresh_token(
        &state.db.write,
        &user.id,
        &refresh_token_hash,
        &expires_at,
        None,
        None,
    )
    .await?;

    let mut cookie = format!(
        "refresh_token={}; HttpOnly; Path=/; Max-Age={}; SameSite=Strict",
        refresh_token, refresh_max_age_seconds
    );
    if state.config.tls.enabled {
        cookie.push_str("; Secure");
    }

    let mut headers = HeaderMap::new();
    let cookie_value = HeaderValue::from_str(&cookie)
        .map_err(|e| AppError::Internal(format!("invalid Set-Cookie value: {e}")))?;
    headers.insert(SET_COOKIE, cookie_value);

    Ok((headers, Json(LoginResponse { access_token })))
}

async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<(HeaderMap, Json<LoginResponse>)> {
    let raw_token = extract_refresh_token(&headers).ok_or(AppError::Unauthorized)?;

    let old_hash = tokens::hash_token(&raw_token);
    let session = sessions::find_by_hash(&state.db.read, &old_hash)
        .await?
        .ok_or(AppError::Unauthorized)?;

    if session.revoked_at.is_some() {
        return Err(AppError::TokenRevoked);
    }

    // Revoke the old token unconditionally (rotation)
    sessions::revoke_refresh_token(&state.db.write, &session.id, "rotated").await?;

    let now = Utc::now();
    let expires_at_dt = chrono::DateTime::parse_from_rfc3339(&session.expires_at)
        .map_err(|_| AppError::Internal("invalid expiry mapping".into()))?;

    if now > expires_at_dt.with_timezone(&Utc) {
        return Err(AppError::SessionExpired);
    }

    let user = users::find_by_username(&state.db.read, &session.user_id)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let auth_cfg = &state.config.auth;
    let access_ttl_minutes = i64::try_from(auth_cfg.access_token_ttl_minutes)
        .map_err(|_| AppError::Internal("ttl bounds error".into()))?;

    let access_token = jwt::issue(
        &user.id,
        &user.username,
        &user.role,
        &auth_cfg.jwt_issuer,
        &auth_cfg.jwt_audience,
        access_ttl_minutes,
        ExposeSecret::expose_secret(&auth_cfg.jwt_signing_key),
    )?;

    let new_refresh_token: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();
    let new_hash = tokens::hash_token(&new_refresh_token);

    sessions::insert_refresh_token(
        &state.db.write,
        &user.id,
        &new_hash,
        &session.expires_at,
        None,
        None,
    )
    .await?;

    let max_age = expires_at_dt.signed_duration_since(now).num_seconds();

    let mut cookie = format!(
        "refresh_token={}; HttpOnly; Path=/; Max-Age={}; SameSite=Strict",
        new_refresh_token, max_age
    );
    if state.config.tls.enabled {
        cookie.push_str("; Secure");
    }

    let mut response_headers = HeaderMap::new();
    let cookie_value = HeaderValue::from_str(&cookie)
        .map_err(|e| AppError::Internal(format!("invalid Set-Cookie value: {e}")))?;
    response_headers.insert(SET_COOKIE, cookie_value);

    Ok((response_headers, Json(LoginResponse { access_token })))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<(HeaderMap, StatusCode)> {
    if let Some(raw_token) = extract_refresh_token(&headers) {
        let hash = tokens::hash_token(&raw_token);
        if let Ok(Some(session)) = sessions::find_by_hash(&state.db.read, &hash).await {
            let _ = sessions::revoke_refresh_token(&state.db.write, &session.id, "logout").await;
        }
    }

    let cookie = "refresh_token=; HttpOnly; Path=/; Max-Age=0; SameSite=Strict";

    let mut response_headers = HeaderMap::new();
    if let Ok(cookie_value) = HeaderValue::from_str(cookie) {
        response_headers.insert(SET_COOKIE, cookie_value);
    }

    Ok((response_headers, StatusCode::NO_CONTENT))
}
