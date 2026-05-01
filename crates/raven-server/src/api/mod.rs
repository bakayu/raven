use axum::{Router, middleware::from_fn, routing::get};
use tower_http::services::ServeDir;

use crate::api::middleware::with_request_context;
use crate::state::AppState;

pub mod agents;
pub mod auth;
pub mod logs;
pub mod metrics;
pub mod middleware;
pub mod users;
pub mod ws;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .nest("/api/auth", auth::router())
        .nest("/api/agents", agents::router())
        .nest("/api/users", users::router())
        .nest("/api/metrics", metrics::router())
        .nest("/api/logs", logs::router())
        .nest("/api/ws", ws::router())
        .fallback_service(ServeDir::new("dashboard/dist").append_index_html_on_directories(true))
        .layer(from_fn(with_request_context))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz() -> &'static str {
    "ok"
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_ok() {
        let state = crate::state::AppState::for_test().await;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readyz_returns_ok() {
        let state = crate::state::AppState::for_test().await;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_namespace_is_wired() {
        let state = crate::state::AppState::for_test().await;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/non-existent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // auth router is mounted; endpoint is not implemented yet
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
