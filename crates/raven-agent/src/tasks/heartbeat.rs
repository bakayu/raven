use std::time::Duration;

use tokio::sync::mpsc;

use crate::AgentEvent;

pub async fn heartbeat_task(tx: mpsc::Sender<AgentEvent>) {
    let mut interval = tokio::time::interval(Duration::from_millis(5_000));

    loop {
        interval.tick().await;

        let _ = tx
            .send(AgentEvent::Heartbeat {
                agent_id: "agent-1".into(),
                hostname: "host-1".into(),
            })
            .await;
    }
}
