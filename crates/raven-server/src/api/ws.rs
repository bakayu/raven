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

                            let payload = serde_json::json!({
                                "hostname": batch.hostname,
                                "app": batch.source,
                                "stream": stream,
                                "path": entry.path,
                                "line": entry.line,
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
