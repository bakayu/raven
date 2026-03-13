/// Re-exports everything generated from raven.proto.
///
/// Usage:
/// ```rust
/// use raven_proto::proto::agent_service_client::AgentServiceClient;
/// use raven_proto::proto::agent_service_server::{AgentService, AgentServiceServer};
/// use raven_proto::proto::{HeartbeatRequest, HeartbeatResponse};
/// ```
pub mod proto {
    tonic::include_proto!("raven_v1");
}
