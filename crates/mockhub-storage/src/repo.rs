//! **Interface storage.** Caller (web/dns/mail/sms) chỉ biết các trait ở đây, không biết SQLite hay Postgres.
//!
//! Thêm dữ liệu mới: (1) thêm kiểu vào `mockhub-core::domain`, (2) thêm một trait `XxxRepo` ở đây
//! và đưa nó vào supertrait của [`Storage`], (3) implement cho từng backend + thêm migration.
use async_trait::async_trait;
use mockhub_core::domain::dns::{DnsRecord, NewDnsRecord};

use crate::error::Result;

#[async_trait]
pub trait DnsRecordRepo: Send + Sync {
    /// Tất cả record, theo thứ tự tạo (id tăng dần).
    async fn list(&self) -> Result<Vec<DnsRecord>>;
    async fn create(&self, new: NewDnsRecord) -> Result<DnsRecord>;
    /// `true` nếu có record bị xoá, `false` nếu `id` không tồn tại (không phải lỗi).
    async fn delete(&self, id: i64) -> Result<bool>;
}

/// Gộp mọi repo lại thành một đối tượng để truyền đi: `Arc<dyn Storage>`.
/// Có blanket impl, nên backend chỉ cần implement từng `XxxRepo`.
pub trait Storage: DnsRecordRepo + 'static {}
impl<T: DnsRecordRepo + 'static> Storage for T {}
