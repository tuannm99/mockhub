use serde::{Deserialize, Serialize};

/// Một DNS record mock đã lưu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub id: i64,
    pub name: String,
    /// Loại record: `A`, `CNAME`, `MX`, ... (chưa ép enum; resolver tự diễn giải).
    pub kind: String,
    pub value: String,
}

/// Dữ liệu để tạo record mới (chưa có `id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewDnsRecord {
    pub name: String,
    pub kind: String,
    pub value: String,
}
