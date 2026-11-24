pub mod smtp;
pub mod imap;
pub mod mime;
pub mod mailbox;

use mockhub_core::{config::MailConfig, errors::Result};

/// Chạy SMTP + IMAP; trả về khi một trong hai dừng.
pub async fn run(host: &str, cfg: &MailConfig) -> Result<()> {
    tokio::try_join!(
        smtp::run(format!("{host}:{}", cfg.smtp_port)),
        imap::run(format!("{host}:{}", cfg.imap_port)),
    )?;
    Ok(())
}
