use anyhow::Context;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_subscriber(service_name: &str, default_level: &str) -> anyhow::Result<()> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    let formatting_layer = BunyanFormattingLayer::new(service_name.to_string(), std::io::stdout);

    Registry::default()
        .with(env_filter)
        .with(JsonStorageLayer)
        .with(formatting_layer)
        .try_init()
        .context("failed to initialize tracing subscriber")?;

    Ok(())
}
