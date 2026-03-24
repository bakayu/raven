use std::path::Path;

use config::{Config, Environment, File};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    pub server: ServerConfig,
    pub metrics: MetricsConfig,
    pub transport: TransportConfig,
    pub logs: Vec<LogSource>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub address: String,
    pub token: String,
    pub tls: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetricsConfig {
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TransportConfig {
    pub batch_size: usize,
    pub flush_interval_seconds: u64,
    pub retry_max_interval_seconds: u64,
    pub wal_max_size_mb: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogSource {
    pub name: String,
    pub path: String,
    pub format: LogFormat,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LogFormat {
    #[default]
    Plain,
    DockerJson,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            metrics: MetricsConfig::default(),
            transport: TransportConfig::default(),
            logs: Vec::new(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:9000".to_string(),
            token: String::new(),
            tls: true,
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            interval_seconds: 10,
        }
    }
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            flush_interval_seconds: 5,
            retry_max_interval_seconds: 60,
            wal_max_size_mb: 100,
        }
    }
}

impl Default for LogSource {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: String::new(),
            format: LogFormat::Plain,
        }
    }
}

impl AgentConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let cfg = Config::builder()
            .set_default("server.address", "127.0.0.1:9090")?
            .set_default("server.token", "")?
            .set_default("server.tls", true)?
            .set_default("metrics.interval_seconds", 10)?
            .set_default("transport.batch_size", 100)?
            .set_default("transport.flush_interval_seconds", 5)?
            .set_default("transport.retry_max_interval_seconds", 60)?
            .set_default("transport.wal_max_size_mb", 100)?
            .add_source(File::from(path).required(false))
            .add_source(Environment::with_prefix("RAVEN").separator("__"))
            .build()?;

        Ok(cfg.try_deserialize()?)
    }
}
