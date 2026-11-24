use serde::Deserialize;

use crate::errors::Result;

/// Biến môi trường trỏ tới file cấu hình (mặc định `config/default.toml`).
pub const CONFIG_PATH_ENV: &str = "MOCKHUB_CONFIG";
pub const DEFAULT_CONFIG_PATH: &str = "config/default.toml";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub mail: MailConfig,
    pub dns: DnsConfig,
    pub sms: SmsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MailConfig {
    pub enabled: bool,
    pub smtp_port: u16,
    pub imap_port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DnsConfig {
    pub enabled: bool,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmsConfig {
    pub enabled: bool,
    pub port: u16,
}

impl Config {
    /// Đọc file TOML, rồi override bằng env `MOCKHUB__<SECTION>__<KEY>`
    /// (vd `MOCKHUB__SERVER__PORT=9000`, `MOCKHUB__DNS__ENABLED=false`).
    pub fn load(path: Option<&str>) -> Result<Self> {
        let path = path
            .map(str::to_owned)
            .or_else(|| std::env::var(CONFIG_PATH_ENV).ok())
            .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_owned());

        let cfg = config::Config::builder()
            .add_source(config::File::with_name(&path))
            .add_source(config::Environment::with_prefix("MOCKHUB").separator("__"))
            .build()?
            .try_deserialize()?;
        Ok(cfg)
    }
}
