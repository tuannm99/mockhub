//! IMAP4rev1 trên các hộp thư của [`crate::mailbox`], đủ cho client thật và cho test đọc thư như
//! ứng dụng. Chỉ có `INBOX`. Không TLS, không `APPEND`/`COPY`; `BODYSTRUCTURE` chỉ là bản
//! text/plain.
//!
//! Lệnh: `CAPABILITY, NOOP, LOGOUT, LOGIN, AUTHENTICATE PLAIN, NAMESPACE, LIST, LSUB, STATUS,
//! SELECT, EXAMINE, CHECK, CLOSE, UNSELECT, EXPUNGE, FETCH, SEARCH, STORE` (cả dạng `UID`), `IDLE`.
//! Không nhận literal `{n}` do client gửi lên.
mod fetch;
mod parse;
mod session;

use std::{sync::Arc, time::Duration};

use mockhub_core::errors::Result;
use mockhub_storage::Storage;
use tokio::net::TcpListener;

use crate::mailbox::{Access, accept_loop};

/// `UIDVALIDITY` đổi mỗi lần chạy: client không được tin UID cache của một DB có thể đã bị xoá.
pub async fn serve(listener: TcpListener, storage: Arc<dyn Storage>, access: Access) -> Result<()> {
    let uid_validity = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(1))
        .as_secs() as u32;
    accept_loop(listener, "imap", move |stream| {
        session::run(stream, storage.clone(), access.clone(), uid_validity)
    })
    .await
}
