use crate::StatsSnapshot;

pub enum AgentEvent {
    Heartbeat { agent_id: String, hostname: String },
    Metrics(StatsSnapshot),
}
