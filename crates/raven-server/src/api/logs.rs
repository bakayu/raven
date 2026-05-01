use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_logs))
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    host: Option<String>,
    app: Option<String>,
    stream: Option<String>,
    search: Option<String>,
    range: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LogRow {
    timestamp: String,
    hostname: String,
    app: String,
    file: String,
    stream: String,
    line: String,
}

async fn get_logs(
    State(state): State<AppState>,
    Query(query): Query<LogsQuery>,
) -> AppResult<Json<Value>> {
    let (from, to) = resolve_time_window(
        query.range.as_deref(),
        query.from.as_deref(),
        query.to.as_deref(),
    )?;
    let limit = query.limit.unwrap_or(1000).min(5000);
    let offset = query.offset.unwrap_or(0);

    let mut sql = format!(
        "SELECT timestamp, hostname, app, file, stream, line FROM logs WHERE timestamp >= toDateTime64('{}', 3, 'UTC') AND timestamp <= toDateTime64('{}', 3, 'UTC')",
        from.to_rfc3339(),
        to.to_rfc3339()
    );

    if let Some(host) = query.host.as_deref() {
        sql.push_str(&format!(" AND hostname = '{}'", escape_sql(host)));
    }
    if let Some(app) = query.app.as_deref() {
        sql.push_str(&format!(" AND app = '{}'", escape_sql(app)));
    }
    if let Some(stream) = query.stream.as_deref() {
        sql.push_str(&format!(" AND stream = '{}'", escape_sql(stream)));
    }
    if let Some(search) = query.search.as_deref() {
        sql.push_str(&format!(" AND line ILIKE '%{}%'", escape_sql(search)));
    }

    sql.push_str(&format!(
        " ORDER BY timestamp ASC LIMIT {} OFFSET {} FORMAT JSONEachRow",
        limit, offset
    ));

    let rows = state.ch_client.query_logs(&sql).await?;
    let parsed: Vec<LogRow> = rows
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()?;

    Ok(Json(serde_json::to_value(parsed)?))
}

fn escape_sql(value: &str) -> String {
    value.replace('\'', "''")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_default_range() {
        let (from, to) = resolve_time_window(Some("5m"), None, None).expect("resolve range");
        assert!(to > from);
    }

    #[test]
    fn rejects_invalid_range() {
        let err = resolve_time_window(Some("bogus"), None, None).expect_err("must fail");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
