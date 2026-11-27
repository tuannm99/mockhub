use serde::{Deserialize, Serialize};

/// TTL của record không khai TTL.
pub const DEFAULT_TTL: u32 = 60;

/// Một DNS record mock đã lưu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub id: i64,
    /// Chữ thường, không có dấu chấm cuối. Có thể là wildcard (`*.app.test`).
    pub name: String,
    /// Loại record: `A`, `CNAME`, `MX`, ... (chưa ép enum; resolver tự diễn giải).
    pub kind: String,
    /// Cú pháp zone file; riêng TXT là chuỗi thô.
    pub value: String,
    pub ttl: u32,
}

/// Dữ liệu để tạo record mới (chưa có `id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewDnsRecord {
    pub name: String,
    #[serde(alias = "type")]
    pub kind: String,
    pub value: String,
    #[serde(default)]
    pub ttl: Option<u32>,
}

/// Tình huống lỗi áp cho một tên.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FaultAction {
    Servfail,
    Refused,
    Nxdomain,
    /// Không trả lời, client tự timeout.
    Timeout,
    /// Trả lời bình thường sau `delay_ms`.
    Delay,
}

impl FaultAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Servfail => "servfail",
            Self::Refused => "refused",
            Self::Nxdomain => "nxdomain",
            Self::Timeout => "timeout",
            Self::Delay => "delay",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Servfail,
            Self::Refused,
            Self::Nxdomain,
            Self::Timeout,
            Self::Delay,
        ]
        .into_iter()
        .find(|a| a.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DnsFault {
    pub id: i64,
    /// Tên hoặc wildcard, cùng dạng với `DnsRecord::name`.
    pub name: String,
    /// `None` = mọi loại record.
    pub kind: Option<String>,
    pub action: FaultAction,
    #[serde(rename = "delayMs")]
    pub delay_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewDnsFault {
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    pub action: FaultAction,
    #[serde(default, rename = "delayMs", alias = "delay_ms")]
    pub delay_ms: u64,
}

/// Một truy vấn DNS đã trả lời (hoặc đã bỏ qua).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DnsQuery {
    pub id: i64,
    /// RFC 3339, giờ UTC.
    pub time: String,
    #[serde(flatten)]
    pub entry: NewDnsQuery,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewDnsQuery {
    /// `udp`, `tcp`, `doh`, `doh-json`, `ui`.
    pub proto: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// `NOERROR`, `NXDOMAIN`, ...; `TIMEOUT` khi không trả lời.
    pub rcode: String,
    /// Số record trong câu trả lời.
    pub answers: u32,
    /// `mock`, `upstream`, `fault`, `none`.
    pub source: String,
}
