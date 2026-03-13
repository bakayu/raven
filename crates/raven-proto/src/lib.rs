/// Re-exports everything generated from raven.proto.
///
/// Usage:
/// ```rust
/// use raven_proto::proto::raven_ingestion_client::RavenIngestionClient;
/// use raven_proto::proto::raven_ingestion_server::{RavenIngestion, RavenIngestionServer};
/// use raven_proto::proto::{HeartbeatRequest, HeartbeatResponse};
/// ```
pub mod proto {
    tonic::include_proto!("raven_v1");
}
