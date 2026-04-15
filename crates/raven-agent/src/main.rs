use std::{path::PathBuf, sync::Arc};

use clap::Parser;
use tracing::info;

use raven_agent::{AgentConfig, init_subscriber, run_agent};

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long, default_value = "/etc/raven/agent.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = Arc::new(AgentConfig::load(&cli.config)?);

    init_subscriber(&cfg.logging.service_name, &cfg.logging.level)?;

    info!(
        server_address = %cfg.server.address,
        tls = cfg.server.tls,
        metrics_interval_seconds = cfg.metrics.interval_seconds,
        heartbeat_interval_seconds = cfg.transport.heartbeat_interval_seconds,
        "agent starting"
    );

    run_agent(cfg).await
}
