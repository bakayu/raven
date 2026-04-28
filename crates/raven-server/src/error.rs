use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;
use tonic::Status;

#[derive(Debug, Error)]
pub enum AppError {
    // Database
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    // Auth
    #[error("invalid or missing token")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("token has been revoked")]
    TokenRevoked,

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("agent not found")]
    AgentNotFound,

    #[error("user not found")]
    UserNotFound,

    #[error("account locked: too many failed attempts")]
    AccountLocked,

    #[error("setup already completed")]
    SetupAlreadyDone,

    #[error("{0} already exists")]
    Conflict(String),

    #[error("jwt error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("session expired")]
    SessionExpired,

    #[error("token already used")] // refresh token rotation
    TokenReused,

    #[error("password hashing error")]
    PasswordHash(argon2::password_hash::Error),

    #[error("oidc error: {0}")]
    Oidc(String),

    #[error("oidc not configured")]
    OidcNotConfigured,

    // Ingest
    #[error("victoria metrics write failed: {0}")]
    VictoriaMetrics(String),

    #[error("clickhouse write failed: {0}")]
    ClickHouse(String),

    // External HTTP (reqwest)
    #[error("http client error: {0}")]
    HttpClient(#[from] reqwest::Error),

    // Serialization
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    // Alert engine
    #[error("notification send failed: {0}")]
    NotificationFailed(String),

    #[error("alert rule invalid: {0}")]
    InvalidAlertRule(String),

    // WebSocket
    #[error("websocket error: {0}")]
    WebSocket(String),

    // Input Validation
    #[error("validation error: {0}")]
    Validation(String),

    // Config
    #[error("configuration error: {0}")]
    Config(String),

    // Internal
    #[error("internal error: {0}")]
    Internal(String),

    // Rate limiting
    #[error("too many requests")]
    RateLimited,
}

// Axum: AppError to HTTP Response
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            // 400
            AppError::Validation(_) => (StatusCode::BAD_REQUEST, self.to_string()),

            // 401
            AppError::Unauthorized
            | AppError::TokenRevoked
            | AppError::SessionExpired
            | AppError::TokenReused
            | AppError::InvalidCredentials => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Jwt(e) => match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
                    (StatusCode::UNAUTHORIZED, "token expired".to_string())
                }
                _ => (StatusCode::UNAUTHORIZED, "invalid token".to_string()),
            },

            // 403
            AppError::AccountLocked | AppError::SetupAlreadyDone | AppError::Forbidden => {
                (StatusCode::FORBIDDEN, self.to_string())
            }

            // 404
            AppError::AgentNotFound | AppError::UserNotFound => {
                (StatusCode::NOT_FOUND, self.to_string())
            }

            // 409
            AppError::Conflict(_) => (StatusCode::CONFLICT, self.to_string()),

            // 422
            AppError::InvalidAlertRule(_) => (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),

            // 429
            AppError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),

            // 503
            AppError::VictoriaMetrics(_)
            | AppError::ClickHouse(_)
            | AppError::HttpClient(_)
            | AppError::NotificationFailed(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "upstream service unavailable".to_string(),
            ),

            // 500 (everything else)
            AppError::Database(_)
            | AppError::Migration(_)
            | AppError::PasswordHash(_)
            | AppError::Oidc(_)
            | AppError::OidcNotConfigured
            | AppError::WebSocket(_)
            | AppError::Serialization(_)
            | AppError::Config(_)
            | AppError::Internal(_) => {
                tracing::error!(error = %self, "internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };

        let body = Json(json!({ "error": message }));
        (status, body).into_response()
    }
}

// Tonic: AppError to gRPC Status
impl From<AppError> for Status {
    fn from(err: AppError) -> Self {
        match err {
            // UNAUTHENTICATED
            AppError::Unauthorized
            | AppError::TokenRevoked
            | AppError::SessionExpired
            | AppError::TokenReused
            | AppError::InvalidCredentials
            | AppError::Jwt(_) => Status::unauthenticated(err.to_string()),

            // PERMISSION_DENIED
            AppError::AccountLocked | AppError::SetupAlreadyDone | AppError::Forbidden => {
                Status::permission_denied(err.to_string())
            }

            // NOT_FOUND
            AppError::AgentNotFound | AppError::UserNotFound => Status::not_found(err.to_string()),

            // ALREADY_EXISTS
            AppError::Conflict(msg) => Status::already_exists(msg),

            // INVALID_ARGUMENT
            AppError::Validation(msg) | AppError::InvalidAlertRule(msg) => {
                Status::invalid_argument(msg)
            }

            // RESOURCE_EXHAUSTED
            AppError::RateLimited => Status::resource_exhausted("too many requests"),

            // UNAVAILABLE
            AppError::VictoriaMetrics(msg)
            | AppError::ClickHouse(msg)
            | AppError::NotificationFailed(msg) => Status::unavailable(msg),

            AppError::HttpClient(e) => {
                tracing::error!(error = %e, "http client error in grpc handler");
                Status::unavailable("upstream service unavailable")
            }

            // INTERNAL — log real cause, never leak it
            AppError::Database(e) => {
                tracing::error!(error = %e, "database error");
                Status::internal("internal error")
            }
            AppError::Migration(e) => {
                tracing::error!(error = %e, "migration error");
                Status::internal("internal error")
            }
            AppError::PasswordHash(e) => {
                tracing::error!(error = %e, "password hash error");
                Status::internal("internal error")
            }
            AppError::Serialization(e) => {
                tracing::error!(error = %e, "serialization error");
                Status::internal("internal error")
            }
            AppError::Oidc(msg)
            | AppError::Config(msg)
            | AppError::Internal(msg)
            | AppError::WebSocket(msg) => {
                tracing::error!(error = %msg, "internal error");
                Status::internal("internal error")
            }
            AppError::OidcNotConfigured => {
                tracing::error!("oidc not configured");
                Status::internal("internal error")
            }
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
