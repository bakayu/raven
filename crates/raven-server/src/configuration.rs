use std::path::Path;

use anyhow::bail;
use config::{Config, Environment, File};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

const DEFAULT_HTTP_LISTEN_ADDR: &str = "0.0.0.0:8080";
const DEFAULT_GRPC_LISTEN_ADDR: &str = "0.0.0.0:9090";
const DEFAULT_PUBLIC_BASE_URL: &str = "http://localhost:8080";

const DEFAULT_TLS_ENABLED: bool = false;
const DEFAULT_TLS_CERT_PATH: &str = "";
const DEFAULT_TLS_KEY_PATH: &str = "";

const DEFAULT_JWT_ISSUER: &str = "raven";
const DEFAULT_JWT_AUDIENCE: &str = "raven-dashboard";
const DEFAULT_ACCESS_TOKEN_TTL_MINUTES: u64 = 15;
const DEFAULT_REFRESH_TOKEN_TTL_DAYS: u64 = 30;

const DEFAULT_GOOGLE_ISSUER: &str = "https://accounts.google.com";
const DEFAULT_GOOGLE_DISCOVERY_URL: &str =
    "https://accounts.google.com/.well-known/openid-configuration";
const DEFAULT_GOOGLE_CALLBACK_PATH: &str = "/api/auth/oidc/google/callback";
const DEFAULT_OIDC_SCOPES: &[&str] = &["openid", "email", "profile"];

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RavenConfig {
    pub server: ServerConfig,
    pub tls: TlsConfig,
    pub auth: AuthConfig,
    pub oidc: OidcConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub http_listen_addr: String,
    pub grpc_listen_addr: String,
    pub public_base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TlsConfig {
    pub enabled: bool,
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub access_token_ttl_minutes: u64,
    pub refresh_token_ttl_days: u64,
    pub jwt_signing_key: SecretString,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OidcConfig {
    pub google: GoogleOidcConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GoogleOidcConfig {
    pub enabled: bool,
    pub issuer: String,
    pub discovery_url: Option<String>,
    pub client_id: String,
    pub client_secret: SecretString,
    pub callback_path: String,
    pub scopes: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_listen_addr: DEFAULT_HTTP_LISTEN_ADDR.to_string(),
            grpc_listen_addr: DEFAULT_GRPC_LISTEN_ADDR.to_string(),
            public_base_url: DEFAULT_PUBLIC_BASE_URL.to_string(),
        }
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_TLS_ENABLED,
            cert_path: DEFAULT_TLS_CERT_PATH.to_string(),
            key_path: DEFAULT_TLS_KEY_PATH.to_string(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_issuer: DEFAULT_JWT_ISSUER.to_string(),
            jwt_audience: DEFAULT_JWT_AUDIENCE.to_string(),
            access_token_ttl_minutes: DEFAULT_ACCESS_TOKEN_TTL_MINUTES,
            refresh_token_ttl_days: DEFAULT_REFRESH_TOKEN_TTL_DAYS,
            jwt_signing_key: SecretString::new(String::new().into()),
        }
    }
}

impl Default for GoogleOidcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            issuer: DEFAULT_GOOGLE_ISSUER.to_string(),
            discovery_url: Some(DEFAULT_GOOGLE_DISCOVERY_URL.to_string()),
            client_id: String::new(),
            client_secret: SecretString::new(String::new().into()),
            callback_path: DEFAULT_GOOGLE_CALLBACK_PATH.to_string(),
            scopes: DEFAULT_OIDC_SCOPES
                .iter()
                .map(|scope| scope.to_string())
                .collect(),
        }
    }
}

impl RavenConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let cfg = Config::builder()
            .set_default("server.http_listen_addr", DEFAULT_HTTP_LISTEN_ADDR)?
            .set_default("server.grpc_listen_addr", DEFAULT_GRPC_LISTEN_ADDR)?
            .set_default("server.public_base_url", DEFAULT_PUBLIC_BASE_URL)?
            .set_default("tls.enabled", DEFAULT_TLS_ENABLED)?
            .set_default("tls.cert_path", DEFAULT_TLS_CERT_PATH)?
            .set_default("tls.key_path", DEFAULT_TLS_KEY_PATH)?
            .set_default("auth.jwt_issuer", DEFAULT_JWT_ISSUER)?
            .set_default("auth.jwt_audience", DEFAULT_JWT_AUDIENCE)?
            .set_default(
                "auth.access_token_ttl_minutes",
                DEFAULT_ACCESS_TOKEN_TTL_MINUTES,
            )?
            .set_default(
                "auth.refresh_token_ttl_days",
                DEFAULT_REFRESH_TOKEN_TTL_DAYS,
            )?
            .set_default("oidc.google.enabled", false)?
            .set_default("oidc.google.issuer", DEFAULT_GOOGLE_ISSUER)?
            .set_default("oidc.google.discovery_url", DEFAULT_GOOGLE_DISCOVERY_URL)?
            .set_default("oidc.google.callback_path", DEFAULT_GOOGLE_CALLBACK_PATH)?
            .set_default(
                "oidc.google.scopes",
                DEFAULT_OIDC_SCOPES
                    .iter()
                    .map(|scope| scope.to_string())
                    .collect::<Vec<_>>(),
            )?
            .add_source(File::from(path).required(false))
            .add_source(
                Environment::with_prefix("RAVEN")
                    .prefix_separator("_")
                    .separator("__")
                    .convert_case(config::Case::Snake)
                    .try_parsing(true),
            )
            .build()?;

        let cfg: RavenConfig = cfg.try_deserialize()?;
        cfg.validate()?;

        Ok(cfg)
    }

    pub fn google_redirect_uri(&self) -> String {
        self.oidc.google.redirect_uri(&self.server.public_base_url)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.server.http_listen_addr.trim().is_empty() {
            bail!("server.http_listen_addr cannot be empty");
        }

        if self.server.grpc_listen_addr.trim().is_empty() {
            bail!("server.grpc_listen_addr cannot be empty");
        }

        if self.server.public_base_url.trim().is_empty() {
            bail!("server.public_base_url cannot be empty");
        }

        if self.tls.enabled {
            if self.tls.cert_path.trim().is_empty() {
                bail!("tls.cert_path cannot be empty when TLS is enabled");
            }
            if self.tls.key_path.trim().is_empty() {
                bail!("tls.key_path cannot be empty when TLS is enabled");
            }
        }

        if self.auth.jwt_issuer.trim().is_empty() {
            bail!("auth.jwt_issuer cannot be empty");
        }

        if self.auth.jwt_audience.trim().is_empty() {
            bail!("auth.jwt_audience cannot be empty");
        }

        if self.auth.jwt_signing_key.expose_secret().trim().is_empty() {
            bail!("auth.jwt_signing_key cannot be empty");
        }

        if self.oidc.google.enabled {
            if self.oidc.google.client_id.trim().is_empty() {
                bail!("oidc.google.client_id cannot be empty when Google OIDC is enabled");
            }

            if self
                .oidc
                .google
                .client_secret
                .expose_secret()
                .trim()
                .is_empty()
            {
                bail!("oidc.google.client_secret cannot be empty when Google OIDC is enabled");
            }

            if self.oidc.google.callback_path.trim().is_empty() {
                bail!("oidc.google.callback_path cannot be empty when Google OIDC is enabled");
            }

            if self.oidc.google.scopes.is_empty() {
                bail!("oidc.google.scopes cannot be empty when Google OIDC is enabled");
            }
        }

        Ok(())
    }
}

impl GoogleOidcConfig {
    pub fn redirect_uri(&self, public_base_url: &str) -> String {
        let base = public_base_url.trim_end_matches('/');
        let path = self.callback_path.trim_start_matches('/');
        format!("{base}/{path}")
    }

    pub fn discovery_or_default(&self) -> String {
        self.discovery_url.clone().unwrap_or_else(|| {
            format!(
                "{}/.well-known/openid-configuration",
                self.issuer.trim_end_matches('/')
            )
        })
    }
}

#[cfg(test)]
impl RavenConfig {
    pub fn for_test() -> Self {
        use secrecy::SecretString;

        RavenConfig {
            server: ServerConfig {
                http_listen_addr: "127.0.0.1:8080".into(),
                grpc_listen_addr: "127.0.0.1:9090".into(),
                public_base_url: "http://localhost:8080".into(),
            },
            tls: TlsConfig {
                enabled: false,
                cert_path: "".into(),
                key_path: "".into(),
            },
            auth: AuthConfig {
                jwt_issuer: "raven-test".into(),
                jwt_audience: "raven-test".into(),
                access_token_ttl_minutes: 15,
                refresh_token_ttl_days: 30,
                jwt_signing_key: SecretString::new("test_signing_key".into()),
            },
            oidc: OidcConfig {
                google: GoogleOidcConfig {
                    enabled: false,
                    issuer: "https://accounts.google.com".into(),
                    discovery_url: None,
                    callback_path: "/api/auth/oidc/google/callback".into(),
                    client_id: "".into(),
                    client_secret: SecretString::new("".into()),
                    scopes: vec![],
                },
            },
        }
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
    fn load_reads_values_from_toml() {
        let path = temp_file("raven_server_values");

        let toml = r#"
[server]
http_listen_addr = "127.0.0.1:8080"
grpc_listen_addr = "127.0.0.1:9090"
public_base_url = "http://localhost:8080"

[tls]
enabled = false
cert_path = ""
key_path = ""

[auth]
jwt_issuer = "raven"
jwt_audience = "raven-dashboard"
access_token_ttl_minutes = 20
refresh_token_ttl_days = 45
jwt_signing_key = "dev-secret-key"

[oidc.google]
enabled = true
issuer = "https://accounts.google.com"
discovery_url = "https://accounts.google.com/.well-known/openid-configuration"
client_id = "google-client-id"
client_secret = "google-client-secret"
callback_path = "/api/auth/oidc/google/callback"
scopes = ["openid", "email", "profile"]
"#;

        fs::write(&path, toml).expect("write test config");

        let cfg = RavenConfig::load(&path).expect("config should load from file");

        assert_eq!(cfg.server.http_listen_addr, "127.0.0.1:8080");
        assert_eq!(cfg.server.grpc_listen_addr, "127.0.0.1:9090");
        assert_eq!(cfg.server.public_base_url, "http://localhost:8080");

        assert!(!cfg.tls.enabled);

        assert_eq!(cfg.auth.jwt_issuer, "raven");
        assert_eq!(cfg.auth.jwt_audience, "raven-dashboard");
        assert_eq!(cfg.auth.access_token_ttl_minutes, 20);
        assert_eq!(cfg.auth.refresh_token_ttl_days, 45);
        assert_eq!(cfg.auth.jwt_signing_key.expose_secret(), "dev-secret-key");

        assert!(cfg.oidc.google.enabled);
        assert_eq!(cfg.oidc.google.issuer, "https://accounts.google.com");
        assert_eq!(
            cfg.oidc.google.discovery_url.as_deref(),
            Some("https://accounts.google.com/.well-known/openid-configuration")
        );
        assert_eq!(cfg.oidc.google.client_id, "google-client-id");
        assert_eq!(
            cfg.oidc.google.client_secret.expose_secret(),
            "google-client-secret"
        );
        assert_eq!(
            cfg.oidc.google.callback_path,
            "/api/auth/oidc/google/callback"
        );
        assert_eq!(
            cfg.oidc.google.scopes,
            vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string()
            ]
        );

        assert_eq!(
            cfg.google_redirect_uri(),
            "http://localhost:8080/api/auth/oidc/google/callback"
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_rejects_enabled_google_oidc_without_credentials() {
        let path = temp_file("raven_server_google_missing_creds");

        let toml = r#"
[auth]
jwt_signing_key = "dev-secret-key"

[oidc.google]
enabled = true
issuer = "https://accounts.google.com"
"#;

        fs::write(&path, toml).expect("write test config");
        let err =
            RavenConfig::load(&path).expect_err("enabled google oidc should require credentials");
        assert!(
            err.to_string().contains("client_id") || err.to_string().contains("client_secret"),
            "unexpected error: {}",
            err
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_rejects_missing_jwt_signing_key_when_file_missing() {
        let path = temp_file("raven_server_missing");
        let err = RavenConfig::load(&path).expect_err("jwt signing key should be required");
        assert!(
            err.to_string().contains("auth.jwt_signing_key"),
            "unexpected error: {}",
            err
        );

        let _ = fs::remove_file(path);
    }
}
