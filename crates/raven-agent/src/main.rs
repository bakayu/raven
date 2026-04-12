use std::{path::PathBuf, sync::Arc};

use clap::Parser;
use tokio::sync::mpsc;
use tracing::info;

use raven_agent::{
    AgentConfig, collector_task, heartbeat_task, init_subscriber, logs_task, transport_task,
};

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
