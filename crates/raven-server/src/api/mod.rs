use axum::{
    Router,
    extract::State,
    http::{
        HeaderValue, Method, StatusCode,
        header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    },
    middleware::from_fn,
    routing::get,
};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
};

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
    let cors_origin = HeaderValue::from_str(&state.config.server.public_base_url).ok();
    let cors = CorsLayer::new()
        .allow_origin(
            cors_origin
                .clone()
                .unwrap_or_else(|| HeaderValue::from_static("*")),
        )
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([AUTHORIZATION, ACCEPT, CONTENT_TYPE]);

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .nest("/api/auth", auth::router())
        .nest("/api/agents", agents::router())
        .nest("/api/users", users::router())
        .nest("/api/metrics", metrics::router())
        .nest("/api/logs", logs::router())
        .nest("/api/ws", ws::router())
        .fallback_service(
            ServeDir::new("dashboard/dist")
                .append_index_html_on_directories(true)
                .not_found_service(ServeFile::new("dashboard/dist/index.html")),
        )
        .layer(cors)
        .layer(from_fn(with_request_context))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(State(state): State<AppState>) -> Result<&'static str, (StatusCode, &'static str)> {
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.db.read)
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "database not ready"))?;

    state
        .vm_client
        .ping()
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "victoriametrics not ready"))?;

    state
        .ch_client
        .ping()
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "clickhouse not ready"))?;

    Ok("ok")
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
    async fn healthz_allows_dashboard_origin() {
        let state = crate::state::AppState::for_test().await;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .header("Origin", "http://localhost:8080")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("http://localhost:8080")
        );
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
