use axum::{Json, Router, routing::post};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::auth::password;
use crate::db::users;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/setup", post(setup))
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

async fn setup(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<SetupRequest>,
) -> AppResult<(StatusCode, Json<SetupResponse>)> {
    // validate input
    if body.username.trim().is_empty() {
        return Err(AppError::Validation("username is required".into()));
    }
    if body.password.len() < 8 {
        return Err(AppError::Validation(
            "password must be at least 8 characters".into(),
        ));
    }

    // reject if any user already exists
    let count = users::count(&state.db.read).await?;
    if count > 0 {
        return Err(AppError::SetupAlreadyDone);
    }

    // hash + insert
    let hash = password::hash(&body.password)?;
    let user_id = users::create(&state.db.write, &body.username, &hash, "admin").await?;

    info!(username = %body.username, user_id = %user_id, "admin account created");

    Ok((
        StatusCode::CREATED,
        Json(SetupResponse {
            message: "admin account created".into(),
            user_id,
        }),
    ))
}
