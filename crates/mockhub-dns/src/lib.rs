pub mod server;
pub mod zone;
pub mod records;
pub mod resolver;

use mockhub_core::{config::DnsConfig, errors::Result};
use tokio::net::UdpSocket;

/// DNS mock — hiện mới bind UDP và bỏ qua gói tin (TODO: dùng hickory-server).
pub async fn run(host: &str, cfg: &DnsConfig) -> Result<()> {
    let addr = format!("{host}:{}", cfg.port);
    let sock = UdpSocket::bind(&addr).await?;
    tracing::info!("dns listening on udp://{addr}");
    let mut buf = [0u8; 512];
    loop {
        let _ = sock.recv_from(&mut buf).await?;
    }
}
