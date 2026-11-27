//! Storage của mockhub: **trait** (`repo`) + **backend** (hiện có `sqlite`).
//!
//! Caller chỉ dùng `Arc<dyn Storage>` lấy từ [`connect`]; backend chọn theo scheme của `database.url`.
//!
//! # Thêm backend (vd Postgres)
//! 1. Thêm feature sqlx tương ứng (`postgres`) vào `[workspace.dependencies] sqlx`.
//! 2. Tạo `src/postgres/mod.rs`: struct bọc `PgPool`, `impl DnsRecordRepo` (và mọi repo khác),
//!    chạy `sqlx::migrate!("migrations/postgres")`; viết migration riêng trong `migrations/postgres/`
//!    (cú pháp khác SQLite: `BIGSERIAL`, `$1` thay cho `?`...).
//! 3. Thêm một nhánh `postgres://` vào [`connect`]. Không code nào ngoài crate này phải đổi.
pub mod error;
pub mod repo;
pub mod sqlite;

use std::sync::Arc;

pub use error::{Result, StorageError};
pub use repo::{DnsFaultRepo, DnsQueryRepo, DnsRecordRepo, MailRepo, MailboxRepo, Storage};

/// Mở storage theo scheme của `url` (và chạy migration).
pub async fn connect(url: &str) -> Result<Arc<dyn Storage>> {
    if url.starts_with("sqlite:") {
        let s = sqlite::SqliteStorage::connect(url).await?;
        tracing::info!("storage: sqlite ({url})");
        return Ok(Arc::new(s));
    }
    Err(StorageError::UnsupportedUrl(url.to_owned()))
}
