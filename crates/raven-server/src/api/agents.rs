use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use hyper::StatusCode;
use rand::prelude::*;
use rand_distr::Alphanumeric;
use serde::{Deserialize, Serialize};

use crate::{
    auth::middleware::{RequireAdmin, RequireAuth},
    db::{
        agents,
        tokens::{self, AgentToken},
    },
    error::{AppError, AppResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_active_agents))
        .route("/tokens", post(generate_token).get(list_tokens))
        .route("/tokens/{id}", delete(revoke_token))
}

#[derive(Debug, Deserialize)]
struct GenerateTokenRequest {
    name: String,
}

#[derive(Debug, Serialize)]
struct GenerateTokenResponse {
    token_id: String,
    raw_token: String,
    message: &'static str,
}

async fn generate_token(
    State(state): State<AppState>,
    RequireAdmin(claims): RequireAdmin,
    Json(body): Json<GenerateTokenRequest>,
) -> AppResult<(StatusCode, Json<GenerateTokenResponse>)> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("token name is required".into()));
    }

    let raw_suffix: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    let raw_token = format!("rvn_{}", raw_suffix);

    let token_id =
        tokens::create_agent_token(&state.db.write, name, &raw_token, &claims.sub).await?;

    Ok((
        StatusCode::CREATED,
        Json(GenerateTokenResponse {
            token_id,
            raw_token,
            message: "Store this token securely, you will not be able to see it again.",
        }),
    ))
}

async fn list_tokens(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
) -> AppResult<Json<Vec<AgentToken>>> {
    let agent_tokens = tokens::list_agent_tokens(&state.db.read, &claims.sub, false).await?;
    Ok(Json(agent_tokens))
}

async fn revoke_token(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Path(token_id): Path<String>,
) -> AppResult<StatusCode> {
    let revoked = tokens::revoke_agent_token(&state.db.write, &token_id, &claims.sub).await?;
    if !revoked {
        return Err(AppError::AgentNotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
struct AgentResponse {
    id: String,
    token_id: String,
    hostname: String,
    ip: Option<String>,
    os: String,
    agent_version: String,
    log_files: serde_json::Value,
    first_seen_at: String,
    last_seen_at: String,
    online: bool,
}

async fn list_active_agents(
    State(state): State<AppState>,
    RequireAuth(_): RequireAuth,
) -> AppResult<Json<Vec<AgentResponse>>> {
    let db_rows = agents::list_agents(&state.db.read).await?;

    let now = chrono::Utc::now();
    let agent_miss_threshold = chrono::Duration::seconds(60);

    let agents = db_rows
        .into_iter()
        .map(|row| {
            // Check DashMap to see if the agent has sent a heartbeat recently
            let (online, last_seen_at) = match state.agents.get(&row.token_id) {
                Some(agent_state) => {
                    let is_online = now.signed_duration_since(agent_state.last_heartbeat)
                        <= agent_miss_threshold;
                    // Format live heartbeat to ISO 8601
                    (
                        is_online,
                        agent_state
                            .last_heartbeat
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    )
                }
                None => (false, row.last_seen_at.clone()),
            };

            let log_files_json: serde_json::Value =
                serde_json::from_str(&row.log_files).unwrap_or_else(|_| serde_json::json!([]));

            AgentResponse {
                id: row.id,
                token_id: row.token_id,
                hostname: row.hostname,
                ip: row.ip,
                os: row.os,
                agent_version: row.agent_version,
                log_files: log_files_json,
                first_seen_at: row.first_seen_at,
                last_seen_at,
                online,
            }
        })
        .collect();

    Ok(Json(agents))
}
