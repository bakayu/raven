use std::time::Duration;

use tokio::sync::mpsc;

use crate::{AgentEvent, Collector};

pub async fn collector_task(tx: mpsc::Sender<AgentEvent>) {
    let mut interval = tokio::time::interval(Duration::from_millis(1_000));
    let mut collector = Collector::new();

    loop {
        interval.tick().await;

        match collector.collect().await {
            Ok(output) => {
                let _ = tx.send(AgentEvent::Metrics(output.telemetry)).await;

                if let Some(inventory) = output.inventory {
                    let _ = tx.send(AgentEvent::Inventory(inventory)).await;
                }
            }
            Err(e) => {
                eprintln!("collector error: {}", e);
            }
        }
    }
}
