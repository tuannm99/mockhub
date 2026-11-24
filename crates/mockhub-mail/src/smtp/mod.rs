use mockhub_core::errors::Result;
use tokio::{io::AsyncWriteExt, net::TcpListener};

/// SMTP mock — hiện mới trả banner rồi đóng kết nối (TODO: nhận mail thật).
pub async fn run(addr: String) -> Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("smtp listening on {addr}");
    loop {
        let (mut sock, _) = listener.accept().await?;
        tokio::spawn(async move {
            let _ = sock.write_all(b"220 mockhub ESMTP\r\n").await;
        });
    }
}
