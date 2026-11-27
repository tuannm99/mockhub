//! Server functions: CODE CHẠY TRÊN SERVER, client gọi như hàm async thường (HTTP/JSON bên dưới).
//! Type request/response dùng chung cho cả hai phía, nên không bao giờ lệch schema. DTO tách khỏi
//! domain để client (wasm) không phụ thuộc `mockhub-core`.
//!
//! Mẫu để nhân bản: thêm `#[get]`/`#[post]` mới ở đây; đọc thì gọi qua `live::use_live` (render
//! được khi SSR, rồi tự làm mới), ghi thì gọi trực tiếp trong event handler.
//! Khi có admin token, mọi route ở đây nằm sau HTTP Basic của `mockhub web`.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// Cổng đang cấu hình; `0` = tắt hoặc không biết (`dx serve`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ports {
    pub smtp: u16,
    pub imap: u16,
    pub pop3: u16,
    pub dns: u16,
    pub api: u16,
    pub sms: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub ports: Ports,
    pub mails: usize,
    pub dns_records: usize,
    pub dns_faults: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub value: String,
    pub ttl: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DnsFault {
    pub id: i64,
    pub name: String,
    /// Rỗng = mọi loại.
    pub kind: String,
    pub action: String,
    pub delay_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DnsQuery {
    pub id: i64,
    pub time: String,
    pub proto: String,
    pub name: String,
    pub kind: String,
    pub rcode: String,
    pub answers: u32,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DnsDashboard {
    pub records: Vec<DnsRecord>,
    pub faults: Vec<DnsFault>,
    /// Mới nhất trước, tối đa 50.
    pub queries: Vec<DnsQuery>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MailRow {
    pub id: i64,
    pub received_at: String,
    pub from: String,
    /// Mọi người nhận (to, cc, bcc).
    pub recipients: Vec<String>,
    pub subject: String,
    pub snippet: String,
    pub read: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MailDetail {
    pub id: i64,
    pub received_at: String,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub raw: String,
    pub headers: String,
    /// Link `http(s)` trong thư, không lặp.
    pub links: Vec<String>,
}

#[get("/api/overview")]
pub async fn overview() -> Result<Overview> {
    let storage = crate::server::storage()?;
    Ok(Overview {
        ports: crate::server::ports()?,
        mails: storage.list_mailbox("*").await?.len(),
        dns_records: storage.list_dns_records().await?.len(),
        dns_faults: storage.list_dns_faults().await?.len(),
    })
}

#[get("/api/dns")]
pub async fn dns_dashboard() -> Result<DnsDashboard> {
    let storage = crate::server::storage()?;
    Ok(DnsDashboard {
        records: storage
            .list_dns_records()
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
        faults: storage
            .list_dns_faults()
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
        queries: storage
            .list_dns_queries(50)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    })
}

#[post("/api/dns/records")]
pub async fn add_dns_record(
    name: String,
    kind: String,
    value: String,
    ttl: Option<u32>,
    replace: bool,
) -> Result<()> {
    let new = mockhub_core::domain::dns::NewDnsRecord {
        name,
        kind,
        value,
        ttl,
    };
    let storage = crate::server::storage()?;
    // Cùng bộ kiểm tra với REST: record sai bị từ chối ngay, không đợi tới lúc có câu hỏi DNS.
    let new = mockhub_dns::records::prepare(new, &storage.list_dns_records().await?, replace)?;
    if replace {
        storage
            .delete_dns_records_named(&new.name, Some(&new.kind))
            .await?;
    }
    storage.create_dns_record(new).await?;
    Ok(())
}

#[post("/api/dns/records/delete")]
pub async fn delete_dns_record(id: i64) -> Result<()> {
    crate::server::storage()?.delete_dns_record(id).await?;
    Ok(())
}

#[post("/api/dns/faults")]
pub async fn add_dns_fault(
    name: String,
    kind: String,
    action: String,
    delay_ms: u64,
) -> Result<()> {
    use mockhub_core::domain::dns::{FaultAction, NewDnsFault};
    let action = FaultAction::parse(&action)
        .ok_or_else(|| std::io::Error::other("loại lỗi không hợp lệ"))?;
    let new = NewDnsFault {
        name,
        kind: Some(kind),
        action,
        delay_ms,
    };
    let new = mockhub_dns::records::prepare_fault(new)?;
    crate::server::storage()?.create_dns_fault(new).await?;
    Ok(())
}

#[post("/api/dns/faults/delete")]
pub async fn delete_dns_fault(id: i64) -> Result<()> {
    crate::server::storage()?.delete_dns_fault(id).await?;
    Ok(())
}

#[post("/api/dns/queries/clear")]
pub async fn clear_dns_queries() -> Result<()> {
    crate::server::storage()?.clear_dns_queries().await?;
    Ok(())
}

/// "Tra thử": đi đúng đường của một câu hỏi DNS thật (fault, record, upstream, log).
#[post("/api/dns/lookup")]
pub async fn dns_lookup(name: String, kind: String) -> Result<String> {
    let qtype = mockhub_dns::records::parse_kind(&kind)?;
    let outcome = crate::server::engine()?.lookup("ui", &name, qtype).await;
    let mockhub_dns::Outcome::Reply(answer) = outcome else {
        return Ok("TIMEOUT: không trả lời, client sẽ tự timeout".to_owned());
    };
    let mut lines = vec![format!(
        "{} (nguồn: {})",
        mockhub_dns::rcode_name(answer.code),
        answer.source.as_str()
    )];
    lines.extend(
        answer
            .records
            .iter()
            .map(|r| format!("{} {} {} {}", r.name, r.ttl, r.record_type(), r.data)),
    );
    Ok(lines.join("\n"))
}

/// Hộp thư của riêng web UI trong `MailboxRepo`: giữ trạng thái đã đọc. IMAP/POP3 không đăng nhập
/// được bằng tên rỗng.
#[cfg(feature = "server")]
const UI_MAILBOX: &str = "";

/// Mới nhất trước. `filter` tìm trong người gửi, người nhận, tiêu đề và nội dung.
#[post("/api/mail/list")]
pub async fn list_mails(filter: String) -> Result<Vec<MailRow>> {
    let storage = crate::server::storage()?;
    let read: std::collections::HashSet<i64> = storage
        .mails_flagged(UI_MAILBOX, "\\Seen")
        .await?
        .into_iter()
        .collect();
    let needle = filter.trim().to_lowercase();
    let mails = storage.list_mails(None, u32::MAX).await?;
    Ok(mails
        .into_iter()
        .filter_map(|m| {
            let recipients = [&m.to[..], &m.cc, &m.bcc].concat();
            let matches = needle.is_empty()
                || [&m.from, &recipients.join(" "), &m.subject, &m.text]
                    .iter()
                    .any(|field| field.to_lowercase().contains(&needle));
            matches.then(|| MailRow {
                id: m.id,
                received_at: m.received_at,
                from: m.from,
                recipients,
                subject: m.subject,
                snippet: m
                    .text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(90)
                    .collect(),
                read: read.contains(&m.id),
            })
        })
        .collect())
}

/// Đọc một thư và đánh dấu đã đọc trên UI. `None`: thư đã bị xoá.
#[post("/api/mail/open")]
pub async fn open_mail(id: i64) -> Result<Option<MailDetail>> {
    let storage = crate::server::storage()?;
    let Some(m) = storage.get_mail(id).await? else {
        return Ok(None);
    };
    storage
        .set_mailbox_flag(UI_MAILBOX, id, "\\Seen", true)
        .await?;
    let headers = m
        .raw
        .split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .to_owned();
    let linked = if m.text.trim().is_empty() {
        m.html.as_deref().unwrap_or_default()
    } else {
        &m.text
    };
    Ok(Some(MailDetail {
        links: crate::links::find(linked),
        headers,
        id: m.id,
        received_at: m.received_at,
        from: m.from,
        to: m.to,
        cc: m.cc,
        bcc: m.bcc,
        subject: m.subject,
        text: m.text,
        html: m.html,
        raw: m.raw,
    }))
}

#[post("/api/mail/clear")]
pub async fn clear_mails() -> Result<()> {
    crate::server::storage()?.clear_mails().await?;
    Ok(())
}

#[cfg(feature = "server")]
impl From<mockhub_core::domain::dns::DnsRecord> for DnsRecord {
    fn from(r: mockhub_core::domain::dns::DnsRecord) -> Self {
        Self {
            id: r.id,
            name: r.name,
            kind: r.kind,
            value: r.value,
            ttl: r.ttl,
        }
    }
}

#[cfg(feature = "server")]
impl From<mockhub_core::domain::dns::DnsFault> for DnsFault {
    fn from(f: mockhub_core::domain::dns::DnsFault) -> Self {
        Self {
            id: f.id,
            name: f.name,
            kind: f.kind.unwrap_or_default(),
            action: f.action.as_str().to_owned(),
            delay_ms: f.delay_ms,
        }
    }
}

#[cfg(feature = "server")]
impl From<mockhub_core::domain::dns::DnsQuery> for DnsQuery {
    fn from(q: mockhub_core::domain::dns::DnsQuery) -> Self {
        let e = q.entry;
        Self {
            id: q.id,
            time: q.time,
            proto: e.proto,
            name: e.name,
            kind: e.kind,
            rcode: e.rcode,
            answers: e.answers,
            source: e.source,
        }
    }
}
