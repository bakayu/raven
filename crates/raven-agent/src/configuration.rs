use std::path::Path;

use config::{Config, Environment, File};
use secrecy::SecretString;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    pub server: ServerConfig,
    pub metrics: MetricsConfig,
    pub transport: TransportConfig,
    pub logging: LoggingConfig,
    pub logs: Vec<LogSource>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub address: String,
    pub token: SecretString,
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
    pub heartbeat_interval_seconds: u64,
    pub channel_capacity: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: String,
    pub service_name: String,
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

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:9090".to_string(),
            token: SecretString::new(String::new().into()),
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
            heartbeat_interval_seconds: 30,
            channel_capacity: 256,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            service_name: "raven-agent".to_string(),
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
            .set_default("transport.heartbeat_interval_seconds", 30)?
            .set_default("transport.channel_capacity", 256)?
            .set_default("logging.level", "info")?
            .set_default("logging.service_name", "raven-agent")?
            .add_source(File::from(path).required(false))
            .add_source(
                Environment::with_prefix("RAVEN")
                    .prefix_separator("_")
                    .separator("__")
                    .convert_case(config::Case::Snake)
                    .try_parsing(true),
            )
            .build()?;

        Ok(cfg.try_deserialize()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();

        std::env::temp_dir().join(format!("{}_{}_{}.toml", name, std::process::id(), nanos))
    }

    #[test]
    fn load_uses_defaults_when_file_missing() {
        let path = temp_file("raven_agent_missing");
        let cfg = AgentConfig::load(&path).expect("config should load defaults");

        assert_eq!(cfg.server.address, "127.0.0.1:9090");
        assert_eq!(cfg.server.token.expose_secret(), "");
        assert!(cfg.server.tls);

        assert_eq!(cfg.metrics.interval_seconds, 10);

        assert_eq!(cfg.transport.batch_size, 100);
        assert_eq!(cfg.transport.flush_interval_seconds, 5);
        assert_eq!(cfg.transport.retry_max_interval_seconds, 60);
        assert_eq!(cfg.transport.wal_max_size_mb, 100);
        assert_eq!(cfg.transport.heartbeat_interval_seconds, 30);
        assert_eq!(cfg.transport.channel_capacity, 256);

        assert_eq!(cfg.logging.level, "info");
        assert_eq!(cfg.logging.service_name, "raven-agent");
        assert!(cfg.logs.is_empty());
    }

    #[test]
    fn load_reads_values_from_toml() {
        let path = temp_file("raven_agent_values");

        let toml = r#"
[server]
address = "10.1.2.3:9090"
token = "rvn_test"
tls = false

[metrics]
interval_seconds = 3

[transport]
batch_size = 7
flush_interval_seconds = 2
retry_max_interval_seconds = 8
wal_max_size_mb = 11
heartbeat_interval_seconds = 4
channel_capacity = 9

[logging]
level = "debug"
service_name = "agent-test"

[[logs]]
name = "nginx"
path = "/var/log/nginx/access.log"
format = "plain"
"#;

        fs::write(&path, toml).expect("write test config");

        let cfg = AgentConfig::load(&path).expect("config should load from file");

        assert_eq!(cfg.server.address, "10.1.2.3:9090");
        assert_eq!(cfg.server.token.expose_secret(), "rvn_test");
        assert!(!cfg.server.tls);

        assert_eq!(cfg.metrics.interval_seconds, 3);

        assert_eq!(cfg.transport.batch_size, 7);
        assert_eq!(cfg.transport.flush_interval_seconds, 2);
        assert_eq!(cfg.transport.retry_max_interval_seconds, 8);
        assert_eq!(cfg.transport.wal_max_size_mb, 11);
        assert_eq!(cfg.transport.heartbeat_interval_seconds, 4);
        assert_eq!(cfg.transport.channel_capacity, 9);

        assert_eq!(cfg.logging.level, "debug");
        assert_eq!(cfg.logging.service_name, "agent-test");

        assert_eq!(cfg.logs.len(), 1);
        assert_eq!(cfg.logs[0].name, "nginx");
        assert_eq!(cfg.logs[0].path, "/var/log/nginx/access.log");
        assert!(matches!(cfg.logs[0].format, LogFormat::Plain));

        let _ = fs::remove_file(path);
    }
}
