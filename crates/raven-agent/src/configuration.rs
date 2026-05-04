use std::path::Path;

use anyhow::bail;
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
    pub tls_domain_name: Option<String>,
    pub tls_ca_cert_path: Option<String>,
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
    pub wal_path: Option<String>,
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
    pub stream: Option<Stream>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LogFormat {
    #[default]
    Plain,
    DockerJson,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:9090".to_string(),
            token: SecretString::new(String::new().into()),
            tls: true,
            tls_domain_name: None,
            tls_ca_cert_path: None,
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
            wal_path: None,
            heartbeat_interval_seconds: 30,
            channel_capacity: 4096,
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
            stream: None,
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
            .set_default("transport.channel_capacity", 4096)?
            .set_default("logging.level", "info")?
            .set_default("logging.service_name", "raven-agent")?
            .add_source(File::from(path).required(false))
            .add_source(
                Environment::with_prefix("RAVEN_AGENT")
                    .prefix_separator("_")
                    .separator("__")
                    .convert_case(config::Case::Snake)
                    .try_parsing(true),
            )
            .build()?;

        let mut cfg: AgentConfig = cfg.try_deserialize()?;

        if cfg.server.tls_domain_name.as_deref() == Some("") {
            cfg.server.tls_domain_name = None;
        }

        if cfg.server.tls_ca_cert_path.as_deref() == Some("") {
            cfg.server.tls_ca_cert_path = None;
        }

        if cfg.transport.wal_path.as_deref() == Some("") {
            cfg.transport.wal_path = None;
        }

        cfg.validate()?;

        Ok(cfg)
    }

    fn validate(&self) -> anyhow::Result<()> {
        for source in &self.logs {
            if source.name.trim().is_empty() {
                bail!("log source name cannot be empty");
            }

            if source.path.trim().is_empty() {
                bail!("log source '{}' path cannot be empty", source.name);
            }

            if matches!(source.format, LogFormat::Plain) && source.stream.is_none() {
                bail!(
                    "log source '{}' with format=plain requires stream=stdout or stream=stderr",
                    source.name
                );
            }
        }

        Ok(())
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
        assert!(cfg.server.tls_domain_name.is_none());
        assert!(cfg.server.tls_ca_cert_path.is_none());

        assert_eq!(cfg.metrics.interval_seconds, 10);

        assert_eq!(cfg.transport.batch_size, 100);
        assert_eq!(cfg.transport.flush_interval_seconds, 5);
        assert_eq!(cfg.transport.retry_max_interval_seconds, 60);
        assert_eq!(cfg.transport.wal_max_size_mb, 100);
        assert!(cfg.transport.wal_path.is_none());
        assert_eq!(cfg.transport.heartbeat_interval_seconds, 30);
        assert_eq!(cfg.transport.channel_capacity, 4096);

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
tls_domain_name = "raven.internal"
tls_ca_cert_path = "/etc/raven/certs/ca.pem"

[metrics]
interval_seconds = 3

[transport]
batch_size = 7
flush_interval_seconds = 2
retry_max_interval_seconds = 8
wal_max_size_mb = 11
wal_path = "/var/lib/raven/test.wal"
heartbeat_interval_seconds = 4
channel_capacity = 9

[logging]
level = "debug"
service_name = "agent-test"

[[logs]]
name = "nginx"
path = "/var/log/nginx/access.log"
format = "plain"
stream = "stdout"
"#;

        fs::write(&path, toml).expect("write test config");

        let cfg = AgentConfig::load(&path).expect("config should load from file");

        assert_eq!(cfg.server.address, "10.1.2.3:9090");
        assert_eq!(cfg.server.token.expose_secret(), "rvn_test");
        assert!(!cfg.server.tls);
        assert_eq!(
            cfg.server.tls_domain_name.as_deref(),
            Some("raven.internal")
        );
        assert_eq!(
            cfg.server.tls_ca_cert_path.as_deref(),
            Some("/etc/raven/certs/ca.pem")
        );

        assert_eq!(cfg.metrics.interval_seconds, 3);

        assert_eq!(cfg.transport.batch_size, 7);
        assert_eq!(cfg.transport.flush_interval_seconds, 2);
        assert_eq!(cfg.transport.retry_max_interval_seconds, 8);
        assert_eq!(cfg.transport.wal_max_size_mb, 11);
        assert_eq!(
            cfg.transport.wal_path.as_deref(),
            Some("/var/lib/raven/test.wal")
        );
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

    #[test]
    fn load_rejects_plain_log_without_stream() {
        let path = temp_file("raven_agent_plain_without_stream");

        let toml = r#"
[[logs]]
name = "nginx"
path = "/var/log/nginx/access.log"
format = "plain"
"#;

        fs::write(&path, toml).expect("write test config");
        let err = AgentConfig::load(&path).expect_err("plain log without stream should fail");
        assert!(
            err.to_string().contains("requires stream"),
            "unexpected error: {}",
            err
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_allows_docker_json_without_stream() {
        let path = temp_file("raven_agent_docker_without_stream");

        let toml = r#"
[[logs]]
name = "my-api"
path = "/var/lib/docker/containers/abc/abc-json.log"
format = "docker-json"
"#;

        fs::write(&path, toml).expect("write test config");
        let cfg = AgentConfig::load(&path).expect("docker-json without stream should be allowed");
        assert_eq!(cfg.logs.len(), 1);
        assert!(cfg.logs[0].stream.is_none());

        let _ = fs::remove_file(path);
    }
}
