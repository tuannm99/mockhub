pub mod api;
pub mod provider;
pub mod delivery;
pub mod webhook;

use mockhub_core::{config::SmsConfig, errors::Result};
use tokio::{io::AsyncWriteExt, net::TcpListener};

/// SMS mock API — hiện mới bind TCP và trả 501 (TODO).
pub async fn run(host: &str, cfg: &SmsConfig) -> Result<()> {
    let addr = format!("{host}:{}", cfg.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("sms listening on http://{addr}");
    loop {
        let (mut sock, _) = listener.accept().await?;
        tokio::spawn(async move {
            let _ = sock
                .write_all(b"HTTP/1.1 501 Not Implemented\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                .await;
        });
    }
}
