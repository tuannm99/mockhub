//! Phần chung của IMAP và POP3. Mỗi địa chỉ nhận là một hộp thư, không cần khai báo: client đăng
//! nhập bằng địa chỉ muốn đọc. User `*` đọc mọi thư. Cờ và xoá là trạng thái riêng của hộp thư
//! (lưu ở `MailboxRepo`); thư vẫn còn với hộp khác, REST và web UI.
use std::{sync::Arc, time::Duration};

use mockhub_core::auth::secret_matches;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub(crate) const MAX_LINE_BYTES: usize = 8 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Ai được đăng nhập.
#[derive(Debug, Clone, Default)]
pub struct Access(Option<Arc<str>>);

impl Access {
    /// `None`: mật khẩu nào cũng được. Có token: mật khẩu phải đúng bằng token.
    pub fn new(token: Option<&str>) -> Self {
        Self(token.filter(|t| !t.is_empty()).map(Into::into))
    }

    /// User rỗng luôn bị từ chối (tên rỗng dành cho web UI).
    pub fn allows(&self, user: &str, password: &str) -> bool {
        !user.trim().is_empty()
            && self
                .0
                .as_deref()
                .is_none_or(|token| secret_matches(token, password))
    }
}

/// Header (kèm dòng trống kết thúc) và phần thân.
pub(crate) fn split_message(raw: &[u8]) -> (Vec<u8>, &[u8]) {
    match raw.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(i) => (raw[..i + 4].to_vec(), &raw[i + 4..]),
        None => {
            let mut head = raw.to_vec();
            head.extend_from_slice(b"\r\n");
            (head, &[])
        }
    }
}

pub(crate) async fn write_all(
    writer: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
) -> std::io::Result<()> {
    tokio::time::timeout(WRITE_TIMEOUT, writer.write_all(bytes))
        .await
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?
}

/// Nhận kết nối mãi, mỗi kết nối một task.
pub(crate) async fn accept_loop<F, Fut>(
    listener: TcpListener,
    name: &'static str,
    handle: F,
) -> mockhub_core::errors::Result<()>
where
    F: Fn(TcpStream) -> Fut,
    Fut: Future<Output = std::io::Result<()>> + Send + 'static,
{
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(got) => got,
            Err(e) => {
                tracing::warn!("{name} accept: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let session = handle(stream);
        tokio::spawn(async move {
            if let Err(e) = session.await {
                tracing::debug!("{name} {peer}: {e}");
            }
        });
    }
}
