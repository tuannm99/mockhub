use mockhub_core::errors::Result;
use tokio::{io::AsyncWriteExt, net::TcpListener};

/// IMAP mock — hiện mới trả banner rồi đóng kết nối (TODO).
pub async fn run(addr: String) -> Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("imap listening on {addr}");
    loop {
        let (mut sock, _) = listener.accept().await?;
        tokio::spawn(async move {
            let _ = sock.write_all(b"* OK mockhub IMAP ready\r\n").await;
        });
    }
}
