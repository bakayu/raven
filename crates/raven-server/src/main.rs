use std::path::PathBuf;
use std::{fs, net::SocketAddr};

use clap::Parser;
use tokio::net::TcpListener;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tracing::info;

use raven_proto::proto::raven_ingestion_server::RavenIngestionServer;
use raven_server::{AppState, RavenConfig, RavenServer, api, init_subscriber};

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

    let state = AppState::new(config).await?;

    let grpc_addr: SocketAddr = state.config.server.grpc_listen_addr.parse()?;
    let grpc_service = RavenIngestionServer::new(RavenServer::new(state.clone()));

    let http_addr: SocketAddr = state.config.server.http_listen_addr.parse()?;
    let http_router = api::router(state.clone());

    let mut grpc_builder = Server::builder();
    if state.config.tls.enabled {
        let cert = fs::read(&state.config.tls.cert_path)?;
        let key = fs::read(&state.config.tls.key_path)?;
        let identity = Identity::from_pem(cert, key);
        grpc_builder = grpc_builder.tls_config(ServerTlsConfig::new().identity(identity))?;
        info!(grpc_addr = %grpc_addr, http_addr = %http_addr, tls = true, "server starting");
    } else {
        info!(grpc_addr = %grpc_addr, http_addr = %http_addr, tls = false, "server starting");
    }

    let tcp_listner = TcpListener::bind(http_addr).await?;

    tokio::try_join!(
        async {
            axum::serve(tcp_listner, http_router)
                .await
                .map_err(anyhow::Error::from)
        },
        async {
            grpc_builder
                .add_service(grpc_service)
                .serve(grpc_addr)
                .await
                .map_err(anyhow::Error::from)
        },
    )?;

    Ok(())
}
