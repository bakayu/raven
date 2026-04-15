mod configuration;
mod event;
mod log_reader;
mod proc_reader;
mod tasks;
mod telemetry;

pub use configuration::AgentConfig;
pub use event::AgentEvent;
pub use log_reader::{LogBatch, LogEntry, LogStream, LogTailer};
pub use proc_reader::{CollectOutput, Collector, InventorySnapshot, StatsSnapshot};
pub use tasks::{
    collector::collector_task, heartbeat::heartbeat_task, logs::logs_task,
    transport::transport_task,
};
pub use telemetry::init_subscriber;

use std::sync::Arc;

use tokio::sync::mpsc;

pub async fn run_agent(cfg: Arc<AgentConfig>) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel(cfg.transport.channel_capacity);

    let heartbeat_task_handle = tokio::spawn(heartbeat_task(tx.clone(), cfg.clone()));
    let collector_task_handle = tokio::spawn(collector_task(tx.clone(), cfg.clone()));
    let transport_task_handle = tokio::spawn(transport_task(rx, cfg.clone()));
    let logs_task_handle = tokio::spawn(logs_task(tx.clone(), cfg.clone()));

    tokio::try_join!(
        heartbeat_task_handle,
        collector_task_handle,
        transport_task_handle,
        logs_task_handle
    )?;

    Ok(())
}
