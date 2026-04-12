use crate::{InventorySnapshot, LogBatch, StatsSnapshot};

pub enum AgentEvent {
    Heartbeat { agent_id: String, hostname: String },
    Metrics(StatsSnapshot),
    Inventory(InventorySnapshot),
    Logs(LogBatch),
}
