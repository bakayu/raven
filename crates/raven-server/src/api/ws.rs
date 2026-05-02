use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;
use chrono::TimeZone;

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
                            let stream = match entry.stream {
                                x if x == raven_proto::proto::LogStream::Stdout as i32 => "stdout",
                                x if x == raven_proto::proto::LogStream::Stderr as i32 => "stderr",
                                _ => "unknown",
                            };

                            let timestamp = entry.timestamp.as_ref().or(batch.sent_at.as_ref())
                                .map(|ts| {
                                    chrono::Utc.timestamp_opt(ts.seconds, ts.nanos as u32)
                                        .single()
                                        .unwrap_or_else(chrono::Utc::now)
                                        .to_rfc3339()
                                })
                                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

                            let payload = serde_json::json!({
                                "hostname": batch.hostname,
                                "app": batch.source,
                                "stream": stream,
                                "file": entry.path,
                                "line": entry.line,
                                "timestamp": timestamp,
                            });

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
