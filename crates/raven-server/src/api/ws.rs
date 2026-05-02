use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use chrono::TimeZone;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new().route("/logs", axum::routing::get(ws_logs))
}

#[derive(Debug, Deserialize)]
struct LogWsQuery {
    host: Option<String>,
    app: Option<String>,
}

async fn ws_logs(
    State(state): State<AppState>,
    Query(query): Query<LogWsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_logs_socket(socket, state, query))
}

async fn handle_logs_socket(socket: WebSocket, state: AppState, query: LogWsQuery) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.log_tx.subscribe();

    loop {
        tokio::select! {
            recv = rx.recv() => {
                match recv {
                    Ok(batch) => {
                        if let Some(host) = query.host.as_deref()
                            && batch.hostname != host {
                                continue;
                            }
                        if let Some(app) = query.app.as_deref()
                            && batch.source != app {
                                continue;
                            }

                        for entry in batch.entries {
                            let payload = format_log_entry(
                                &batch.hostname,
                                &batch.source,
                                batch.sent_at.as_ref(),
                                &entry,
                            );

                            if sender.send(Message::Text(payload.to_string().into())).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return,
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => return,
                }
            }
        }
    }
}

fn format_log_entry(
    hostname: &str,
    app: &str,
    batch_sent_at: Option<&prost_types::Timestamp>,
    entry: &raven_proto::proto::LogEntry,
) -> serde_json::Value {
    let stream = match entry.stream {
        x if x == raven_proto::proto::LogStream::Stdout as i32 => "stdout",
        x if x == raven_proto::proto::LogStream::Stderr as i32 => "stderr",
        _ => "unknown",
    };

    let timestamp = entry
        .timestamp
        .as_ref()
        .or(batch_sent_at)
        .map(|ts| {
            chrono::Utc
                .timestamp_opt(ts.seconds, ts.nanos as u32)
                .single()
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339()
        })
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    serde_json::json!({
        "hostname": hostname,
        "app": app,
        "stream": stream,
        "file": entry.path,
        "line": entry.line,
        "timestamp": timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Timestamp;
    use raven_proto::proto::{LogEntry, LogStream};

    #[test]
    fn format_log_entry_works() {
        let entry = LogEntry {
            source: "app".into(),
            path: "/var/log/app.log".into(),
            line: "hello".into(),
            stream: LogStream::Stdout as i32,
            timestamp: Some(Timestamp {
                seconds: 1700000000,
                nanos: 0,
            }),
        };

        let val = format_log_entry("web-1", "app", None, &entry);
        assert_eq!(val["hostname"], "web-1");
        assert_eq!(val["app"], "app");
        assert_eq!(val["stream"], "stdout");
        assert_eq!(val["file"], "/var/log/app.log");
        assert_eq!(val["line"], "hello");
        assert_eq!(val["timestamp"], "2023-11-14T22:13:20+00:00");
    }

    #[test]
    fn format_log_entry_falls_back_to_batch_timestamp() {
        let entry = LogEntry {
            source: "app".into(),
            path: "/var/log/app.log".into(),
            line: "hello".into(),
            stream: LogStream::Stdout as i32,
            timestamp: None,
        };
        let batch_ts = Timestamp {
            seconds: 1700000000,
            nanos: 0,
        };

        let val = format_log_entry("web-1", "app", Some(&batch_ts), &entry);
        assert_eq!(val["timestamp"], "2023-11-14T22:13:20+00:00");
    }
}
