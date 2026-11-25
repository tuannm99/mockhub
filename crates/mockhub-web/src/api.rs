//! Server functions: CODE CHẠY TRÊN SERVER, client gọi như hàm async thường (HTTP/JSON bên dưới).
//! Type request/response dùng chung cho cả hai phía, nên không bao giờ lệch schema.
//!
//! Mẫu để nhân bản: thêm `#[get]`/`#[post]` mới ở đây, gọi từ page bằng `use_server_future`
//! (đọc, render được cả khi SSR) hoặc gọi trực tiếp trong event handler (ghi).
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub value: String,
}

#[get("/api/dns/records")]
pub async fn list_dns_records() -> Result<Vec<DnsRecord>> {
    let rows = crate::server::storage()?.list().await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

#[post("/api/dns/records")]
pub async fn add_dns_record(name: String, kind: String, value: String) -> Result<DnsRecord> {
    let new = mockhub_core::domain::dns::NewDnsRecord { name, kind, value };
    Ok(crate::server::storage()?.create(new).await?.into())
}

#[post("/api/dns/records/delete")]
pub async fn delete_dns_record(id: i64) -> Result<()> {
    crate::server::storage()?.delete(id).await?;
    Ok(())
}

/// Domain -> DTO. DTO tách riêng để client (wasm) không phải phụ thuộc `mockhub-core`.
#[cfg(feature = "server")]
impl From<mockhub_core::domain::dns::DnsRecord> for DnsRecord {
    fn from(r: mockhub_core::domain::dns::DnsRecord) -> Self {
        Self { id: r.id, name: r.name, kind: r.kind, value: r.value }
    }
}
