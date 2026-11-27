//! Mail mock: SMTP nhận và lưu thư; IMAP và POP3 đọc cùng kho đó.
pub mod imap;
pub mod mailbox;
pub mod mime;
pub mod pop3;
pub mod smtp;

use std::sync::Arc;

use mockhub_core::{config::MailConfig, errors::Result};
use mockhub_storage::Storage;
use tokio::net::TcpListener;

async fn bind(host: &str, port: u16, name: &str) -> Result<Option<TcpListener>> {
    if port == 0 {
        tracing::info!("{name} disabled");
        return Ok(None);
    }
    let listener = TcpListener::bind(format!("{host}:{port}")).await?;
    tracing::info!("{name} listening on {host}:{port}");
    Ok(Some(listener))
}

/// Chạy SMTP + IMAP + POP3; trả về khi một trong số đó dừng. `token`: nếu có, IMAP/POP3 đòi mật
/// khẩu đúng bằng nó (SMTP vẫn nhận mọi tài khoản: nó chỉ nhận thư).
pub async fn run(
    host: &str,
    cfg: &MailConfig,
    storage: Arc<dyn Storage>,
    token: Option<&str>,
) -> Result<()> {
    let smtp_listener = TcpListener::bind(format!("{host}:{}", cfg.smtp_port)).await?;
    tracing::info!("smtp listening on {host}:{}", cfg.smtp_port);
    let imap_listener = bind(host, cfg.imap_port, "imap").await?;
    let pop3_listener = bind(host, cfg.pop3_port, "pop3").await?;
    let access = mailbox::Access::new(token);
    let imap = async {
        match imap_listener {
            Some(l) => imap::serve(l, storage.clone(), access.clone()).await,
            None => std::future::pending().await,
        }
    };
    let pop3 = async {
        match pop3_listener {
            Some(l) => pop3::serve(l, storage.clone(), access.clone()).await,
            None => std::future::pending().await,
        }
    };
    tokio::try_join!(
        smtp::serve(smtp_listener, storage.clone(), cfg.capacity),
        imap,
        pop3,
    )?;
    Ok(())
}
