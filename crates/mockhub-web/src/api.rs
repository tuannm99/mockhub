//! Server functions: CODE CHẠY TRÊN SERVER, client gọi như hàm async thường (HTTP/JSON bên dưới).
//! Type request/response dùng chung cho cả hai phía, nên không bao giờ lệch schema.
//!
//! Mẫu để nhân bản: thêm `#[get]`/`#[post]` mới ở đây, gọi từ page bằng `use_server_future`
//! (đọc, render được cả khi SSR) hoặc gọi trực tiếp trong event handler (ghi).
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub id: u32,
    pub name: String,
    pub kind: String,
    pub value: String,
}

#[get("/api/dns/records")]
pub async fn list_dns_records() -> Result<Vec<DnsRecord>> {
    Ok(store::list())
}

#[post("/api/dns/records")]
pub async fn add_dns_record(name: String, kind: String, value: String) -> Result<DnsRecord> {
    Ok(store::add(name, kind, value))
}

#[post("/api/dns/records/delete")]
pub async fn delete_dns_record(id: u32) -> Result<()> {
    store::remove(id);
    Ok(())
}

/// Store in-memory tạm để chạy được mẫu. Thay bằng `mockhub-storage`/`mockhub-dns`.
#[cfg(feature = "server")]
mod store {
    use super::DnsRecord;
    use std::sync::{LazyLock, Mutex};

    static RECORDS: LazyLock<Mutex<Vec<DnsRecord>>> = LazyLock::new(|| {
        let r = |id, name: &str, kind: &str, value: &str| DnsRecord {
            id,
            name: name.into(),
            kind: kind.into(),
            value: value.into(),
        };
        Mutex::new(vec![
            r(1, "app.test", "A", "127.0.0.1"),
            r(2, "www.app.test", "CNAME", "app.test"),
            r(3, "app.test", "MX", "10 mail.app.test"),
        ])
    });

    pub fn list() -> Vec<DnsRecord> {
        RECORDS.lock().unwrap().clone()
    }

    pub fn add(name: String, kind: String, value: String) -> DnsRecord {
        let mut rows = RECORDS.lock().unwrap();
        let id = rows.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        let rec = DnsRecord { id, name, kind, value };
        rows.push(rec.clone());
        rec
    }

    pub fn remove(id: u32) {
        RECORDS.lock().unwrap().retain(|r| r.id != id);
    }
}
