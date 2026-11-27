//! **Interface storage.** Caller (web/dns/mail/sms) chỉ biết các trait ở đây, không biết SQLite hay Postgres.
//!
//! Thêm dữ liệu mới: (1) thêm kiểu vào `mockhub-core::domain`, (2) thêm một trait `XxxRepo` ở đây
//! và đưa nó vào supertrait của [`Storage`], (3) implement cho từng backend + thêm migration.
//!
//! Tên method phải duy nhất giữa các repo, vì `dyn Storage` gộp tất cả.
use async_trait::async_trait;
use mockhub_core::domain::{
    dns::{DnsFault, DnsQuery, DnsRecord, NewDnsFault, NewDnsQuery, NewDnsRecord},
    mail::{Mail, MailboxEntry, NewMail},
};

use crate::error::Result;

#[async_trait]
pub trait DnsRecordRepo: Send + Sync {
    /// Tất cả record, theo thứ tự tạo (id tăng dần).
    async fn list_dns_records(&self) -> Result<Vec<DnsRecord>>;
    async fn create_dns_record(&self, new: NewDnsRecord) -> Result<DnsRecord>;
    /// `true` nếu có record bị xoá, `false` nếu `id` không tồn tại (không phải lỗi).
    async fn delete_dns_record(&self, id: i64) -> Result<bool>;
    /// Xoá theo tên (so khớp nguyên văn với tên đã lưu), tuỳ chọn thêm loại. Trả số record đã xoá.
    async fn delete_dns_records_named(&self, name: &str, kind: Option<&str>) -> Result<u64>;
    async fn clear_dns_records(&self) -> Result<()>;
}

#[async_trait]
pub trait DnsFaultRepo: Send + Sync {
    async fn list_dns_faults(&self) -> Result<Vec<DnsFault>>;
    async fn create_dns_fault(&self, new: NewDnsFault) -> Result<DnsFault>;
    async fn delete_dns_fault(&self, id: i64) -> Result<bool>;
    async fn clear_dns_faults(&self) -> Result<()>;
}

#[async_trait]
pub trait MailRepo: Send + Sync {
    /// Lưu thư rồi bỏ các thư cũ nhất vượt quá `keep`.
    async fn save_mail(&self, new: NewMail, keep: u32) -> Result<Mail>;
    /// Mới nhất trước. `recipient` lọc theo địa chỉ nhận (to/cc/bcc), không phân biệt hoa thường.
    async fn list_mails(&self, recipient: Option<&str>, limit: u32) -> Result<Vec<Mail>>;
    async fn get_mail(&self, id: i64) -> Result<Option<Mail>>;
    async fn clear_mails(&self) -> Result<()>;
    /// Nguyên văn đúng từng byte (`Mail::raw` đã đổi sang UTF-8, có thể mất mát).
    async fn get_mail_raw(&self, id: i64) -> Result<Option<Vec<u8>>>;
    /// `id` mà thư kế tiếp sẽ nhận (IMAP `UIDNEXT`).
    async fn next_mail_id(&self) -> Result<i64>;
}

/// Trạng thái đọc/cờ/xoá của từng hộp thư. Không đụng vào thư: REST và hộp khác vẫn thấy.
/// `mailbox` không phân biệt hoa thường; `*` thấy mọi thư.
#[async_trait]
pub trait MailboxRepo: Send + Sync {
    /// Thư hộp này thấy (người nhận trong phong bì, chưa bị nó xoá), cũ nhất trước.
    async fn list_mailbox(&self, mailbox: &str) -> Result<Vec<MailboxEntry>>;
    async fn mailbox_flags(&self, mailbox: &str, id: i64) -> Result<Vec<String>>;
    /// Id các thư mà hộp này đã gắn `flag`.
    async fn mails_flagged(&self, mailbox: &str, flag: &str) -> Result<Vec<i64>>;
    /// Bật/tắt một cờ. Thư không còn thì bỏ qua.
    async fn set_mailbox_flag(&self, mailbox: &str, id: i64, flag: &str, on: bool) -> Result<()>;
    async fn hide_from_mailbox(&self, mailbox: &str, ids: &[i64]) -> Result<()>;
}

#[async_trait]
pub trait DnsQueryRepo: Send + Sync {
    /// Ghi một truy vấn rồi chỉ giữ `keep` dòng mới nhất.
    async fn log_dns_query(&self, new: NewDnsQuery, keep: u32) -> Result<()>;
    /// Mới nhất trước.
    async fn list_dns_queries(&self, limit: u32) -> Result<Vec<DnsQuery>>;
    async fn clear_dns_queries(&self) -> Result<()>;
}

/// Gộp mọi repo lại thành một đối tượng để truyền đi: `Arc<dyn Storage>`.
/// Có blanket impl, nên backend chỉ cần implement từng `XxxRepo`.
pub trait Storage:
    DnsRecordRepo + DnsFaultRepo + DnsQueryRepo + MailRepo + MailboxRepo + 'static
{
}
impl<T> Storage for T where
    T: DnsRecordRepo + DnsFaultRepo + DnsQueryRepo + MailRepo + MailboxRepo + 'static
{
}
