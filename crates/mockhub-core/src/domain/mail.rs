use serde::{Deserialize, Serialize};

/// Một thư đã nhận qua SMTP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mail {
    pub id: i64,
    /// RFC 3339, giờ UTC.
    pub received_at: String,
    /// `MAIL FROM` của phiên SMTP.
    pub from: String,
    /// Người nhận trong phong bì (`RCPT TO`) có mặt ở header `To`.
    pub to: Vec<String>,
    /// Người nhận trong phong bì có mặt ở header `Cc`.
    pub cc: Vec<String>,
    /// Người nhận trong phong bì không có ở `To` lẫn `Cc`: BCC không xuất hiện trong header.
    pub bcc: Vec<String>,
    /// Đã giải mã encoded-word.
    pub subject: String,
    /// Phần text đã giải mã.
    pub text: String,
    pub html: Option<String>,
    /// Nguyên văn thư.
    pub raw: String,
}

/// Thư chưa lưu (chưa có `id`, `received_at`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewMail {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub raw: Vec<u8>,
}

/// Một thư như một hộp thư (IMAP/POP3) nhìn thấy. Nội dung đọc riêng khi cần.
#[derive(Debug, Clone, PartialEq)]
pub struct MailboxEntry {
    /// Cũng là UID của IMAP: id không bao giờ được dùng lại.
    pub id: i64,
    /// Số byte của nguyên văn.
    pub size: u64,
    /// Cờ riêng của hộp thư này (`\Seen`, `\Deleted`...), đã sắp xếp.
    pub flags: Vec<String>,
}
