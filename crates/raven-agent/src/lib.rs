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
