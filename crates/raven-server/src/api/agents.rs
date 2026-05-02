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
    auth::middleware::RequireAuth,
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
        .route("/{id}", delete(remove_agent))
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

/// Any authenticated user can create agent tokens (scoped to their own account).
async fn generate_token(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
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

/// Admin sees all tokens; member sees only their own.
async fn list_tokens(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
) -> AppResult<Json<Vec<AgentToken>>> {
    let agent_tokens = if claims.role == "admin" {
        tokens::list_all_agent_tokens(&state.db.read, false).await?
    } else {
        tokens::list_agent_tokens(&state.db.read, &claims.sub, false).await?
    };
    Ok(Json(agent_tokens))
}

async fn revoke_token(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Path(token_id): Path<String>,
) -> AppResult<StatusCode> {
    let revoked = if claims.role == "admin" {
        tokens::revoke_agent_token_any(&state.db.write, &token_id).await?
    } else {
        tokens::revoke_agent_token(&state.db.write, &token_id, &claims.sub).await?
    };
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

/// Admin sees all agents; member sees only agents linked to their own tokens.
async fn list_active_agents(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
) -> AppResult<Json<Vec<AgentResponse>>> {
    let db_rows = if claims.role == "admin" {
        agents::list_agents(&state.db.read).await?
    } else {
        agents::list_agents_for_user(&state.db.read, &claims.sub).await?
    };

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

/// Remove an agent (admin can delete any, member can delete only their own).
async fn remove_agent(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Path(agent_id): Path<String>,
) -> AppResult<StatusCode> {
    let deleted = if claims.role == "admin" {
        agents::delete_agent_any(&state.db.write, &agent_id).await?
    } else {
        agents::delete_agent(&state.db.write, &agent_id, &claims.sub).await?
    };
    if !deleted {
        return Err(AppError::AgentNotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token_request_deserializes() {
        let json = r#"{"name": "my-agent"}"#;
        let req: GenerateTokenRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "my-agent");
    }

    #[test]
    fn agent_response_serializes() {
        let resp = AgentResponse {
            id: "1".into(),
            token_id: "2".into(),
            hostname: "host".into(),
            ip: Some("1.2.3.4".into()),
            os: "linux".into(),
            agent_version: "1.0".into(),
            log_files: serde_json::json!(["/var/log/app.log"]),
            first_seen_at: "2023-01-01".into(),
            last_seen_at: "2023-01-02".into(),
            online: true,
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["hostname"], "host");
        assert_eq!(val["online"], true);
        assert_eq!(val["log_files"][0], "/var/log/app.log");
    }
}
