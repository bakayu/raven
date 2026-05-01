use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_metrics))
}

#[derive(Debug, Deserialize)]
struct MetricsQuery {
    host: Option<String>,
    metric: Option<String>,
    range: Option<String>,
    from: Option<String>,
    to: Option<String>,
    step: Option<String>,
}

async fn get_metrics(
    State(state): State<AppState>,
    Query(query): Query<MetricsQuery>,
) -> AppResult<Json<Value>> {
    let metric = query.metric.as_deref().unwrap_or("cpu");
    let host = query.host.as_deref().unwrap_or("all");
    let (from, to) = resolve_time_window(
        query.range.as_deref(),
        query.from.as_deref(),
        query.to.as_deref(),
    )?;
    let step = query
        .step
        .as_deref()
        .unwrap_or_else(|| default_step(&from, &to));

    let vm_query = format!(
        "raven_{}_usage_percent{{hostname=\"{}\"}}",
        metric,
        host.replace('"', "\\\"")
    );
    let response = state.vm_client.query_range(&vm_query).await?;

    let mut payload = response;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("from".into(), Value::String(from.to_rfc3339()));
        obj.insert("to".into(), Value::String(to.to_rfc3339()));
        obj.insert("step".into(), Value::String(step.to_string()));
    }

    Ok(Json(payload))
}

fn resolve_time_window(
    range: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> AppResult<(DateTime<Utc>, DateTime<Utc>)> {
    if let (Some(from), Some(to)) = (from, to) {
        let from = DateTime::parse_from_rfc3339(from)
            .map_err(|e| AppError::Validation(format!("invalid from timestamp: {e}")))?
            .with_timezone(&Utc);
        let to = DateTime::parse_from_rfc3339(to)
            .map_err(|e| AppError::Validation(format!("invalid to timestamp: {e}")))?
            .with_timezone(&Utc);
        return Ok((from, to));
    }

    let to = Utc::now();
    let from = match range.unwrap_or("1h") {
        "5m" => to - Duration::minutes(5),
        "15m" => to - Duration::minutes(15),
        "1h" => to - Duration::hours(1),
        "6h" => to - Duration::hours(6),
        "24h" => to - Duration::hours(24),
        "7d" => to - Duration::days(7),
        other => return Err(AppError::Validation(format!("unsupported range: {other}"))),
    };

    Ok((from, to))
}

fn default_step(from: &DateTime<Utc>, to: &DateTime<Utc>) -> &'static str {
    let span = to.signed_duration_since(*from);
    if span <= Duration::minutes(15) {
        "5s"
    } else if span <= Duration::hours(1) {
        "15s"
    } else if span <= Duration::hours(6) {
        "30s"
    } else if span <= Duration::hours(24) {
        "1m"
    } else {
        "5m"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_shorthand_range() {
        let (from, to) = resolve_time_window(Some("1h"), None, None).expect("resolve range");
        assert!(to > from);
        assert_eq!(default_step(&from, &to), "15s");
    }

    #[test]
    fn resolves_explicit_window() {
        let (from, to) = resolve_time_window(
            None,
            Some("2026-03-01T00:00:00Z"),
            Some("2026-03-01T01:00:00Z"),
        )
        .expect("resolve explicit window");

        assert_eq!(from.to_rfc3339(), "2026-03-01T00:00:00+00:00");
        assert_eq!(to.to_rfc3339(), "2026-03-01T01:00:00+00:00");
    }
}
