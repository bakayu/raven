use std::fs;
use std::path::PathBuf;

use clap::Parser;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tracing::info;

use raven_proto::proto::raven_ingestion_server::RavenIngestionServer;
use raven_server::{
    AppState, Db, RavenConfig, RavenServer, init_subscriber, tokens::seed_dev_token,
};

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long, default_value = "/etc/raven/server.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    init_subscriber("raven-server", "info")?;

    let config = RavenConfig::load(&cli.config)?;

    let db = Db::connect(&config.database.sqlite_path).await?;
    sqlx::migrate!("./migrations").run(&db.write).await?;
    seed_dev_token(&db.write, "rvn_dev_token").await?;

    let state = AppState::new(config, db);

    let addr: std::net::SocketAddr = state.config.server.grpc_listen_addr.parse()?;
    let service = RavenIngestionServer::new(RavenServer::new(state.clone()));

    let mut builder = Server::builder();

    if state.config.tls.enabled {
        let cert = fs::read(&state.config.tls.cert_path)?;
        let key = fs::read(&state.config.tls.key_path)?;
        let identity = Identity::from_pem(cert, key);
        builder = builder.tls_config(ServerTlsConfig::new().identity(identity))?;
        info!(listen_addr = %addr, tls = true, "server starting");
    } else {
        info!(listen_addr = %addr, tls = false, "server starting");
    }

    builder.add_service(service).serve(addr).await?;
    Ok(())
}
