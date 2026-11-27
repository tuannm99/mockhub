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
    pub api: ApiConfig,
    #[serde(default)]
    pub auth: AuthConfig,
}

#[derive(Clone, Default, Deserialize)]
pub struct AuthConfig {
    /// Đặt thì REST `/v1`, web UI, IMAP và POP3 đòi token này. Rỗng = không xác thực.
    #[serde(default)]
    pub admin_token: Option<String>,
}

impl AuthConfig {
    pub fn token(&self) -> Option<&str> {
        self.admin_token.as_deref().filter(|t| !t.is_empty())
    }
}

/// Không để token lọt vào log khi in cả `Config`.
impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.token().map_or("unset", |_| "set");
        write!(f, "AuthConfig {{ admin_token: {state} }}")
    }
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
    /// `0` = tắt IMAP.
    pub imap_port: u16,
    /// `0` = tắt POP3.
    #[serde(default = "default_pop3_port")]
    pub pop3_port: u16,
    /// Số thư giữ lại; quá số này thì bỏ thư cũ nhất.
    #[serde(default = "default_mail_capacity")]
    pub capacity: u32,
}

fn default_pop3_port() -> u16 {
    1110
}

fn default_mail_capacity() -> u32 {
    500
}

#[derive(Debug, Clone, Deserialize)]
pub struct DnsConfig {
    pub enabled: bool,
    pub port: u16,
    /// File JSON record/fault nạp khi DB chưa có record nào. `None` hoặc rỗng = không nạp.
    #[serde(default)]
    pub seed_file: Option<String>,
    /// Resolver thật (`ip` hoặc `ip:port`) cho tên mock không sở hữu. `None`/rỗng = `NXDOMAIN`.
    #[serde(default)]
    pub upstream: Option<String>,
}

/// REST cho script/test và DNS-over-HTTPS (JSON). Cổng riêng, không cần client web đã build.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
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
