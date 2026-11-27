//! DNS mock: UDP + TCP, record lấy từ storage, có tình huống lỗi giả lập.
//! Chỉ cho dev/test; tên không có record trả `NXDOMAIN`, trừ khi cấu hình `dns.upstream`.
//! Mọi đường hỏi (UDP, TCP, DoH, UI) đi qua [`Engine::lookup`] nên trả cùng kết quả và cùng được ghi log.
pub mod records;
pub mod resolver;
pub mod seed;
pub mod server;
pub mod upstream;
pub mod zone;

use std::{sync::Arc, time::Duration};

use hickory_proto::{op::ResponseCode, rr::RecordType};
use mockhub_core::{
    config::DnsConfig,
    domain::dns::{FaultAction, NewDnsQuery},
    errors::Result,
};
use mockhub_storage::Storage;
use tokio::net::{TcpListener, UdpSocket};

pub use resolver::{Answer, Source};
pub use upstream::Upstream;

/// Số truy vấn giữ trong log.
pub const QUERY_LOG_LIMIT: u32 = 500;

/// Trần cho fault `delay`.
const MAX_DELAY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub enum Outcome {
    Reply(Answer),
    /// Không trả lời: client tự timeout.
    Drop,
}

fn failed(code: ResponseCode, source: Source) -> Answer {
    Answer {
        code,
        records: Vec::new(),
        source,
        unowned: None,
    }
}

/// Tên rcode như `dig` in.
pub fn rcode_name(code: ResponseCode) -> String {
    match code {
        ResponseCode::NoError => "NOERROR".into(),
        ResponseCode::FormErr => "FORMERR".into(),
        ResponseCode::ServFail => "SERVFAIL".into(),
        ResponseCode::NXDomain => "NXDOMAIN".into(),
        ResponseCode::NotImp => "NOTIMP".into(),
        ResponseCode::Refused => "REFUSED".into(),
        other => format!("RCODE{}", u16::from(other)),
    }
}

/// Storage + resolver thật (nếu có).
#[derive(Clone)]
pub struct Engine {
    pub storage: Arc<dyn Storage>,
    upstream: Option<Upstream>,
}

impl Engine {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            upstream: None,
        }
    }

    /// `None` hoặc chuỗi rỗng = không forward.
    pub fn with_upstream(mut self, address: Option<&str>) -> Self {
        self.upstream = address.and_then(Upstream::new);
        self
    }

    pub fn upstream(&self) -> Option<&str> {
        self.upstream.as_ref().map(Upstream::address)
    }

    /// Trả lời một câu hỏi: fault trước, rồi record, rồi resolver thật. `proto` chỉ để ghi log.
    pub async fn lookup(&self, proto: &str, raw_name: &str, qtype: RecordType) -> Outcome {
        let name = records::normalize_name(raw_name);
        let outcome = match &name {
            Ok(name) => self.answer(name, qtype).await,
            Err(_) => Outcome::Reply(failed(ResponseCode::FormErr, Source::None)),
        };
        let (rcode, answers, source) = match &outcome {
            Outcome::Reply(a) => (rcode_name(a.code), a.records.len(), a.source),
            Outcome::Drop => ("TIMEOUT".to_owned(), 0, Source::Fault),
        };
        let entry = NewDnsQuery {
            proto: proto.to_owned(),
            name: name.unwrap_or_else(|_| raw_name.chars().take(253).collect()),
            kind: qtype.to_string(),
            rcode,
            answers: answers as u32,
            source: source.as_str().to_owned(),
        };
        tracing::info!(proto, name = %entry.name, %qtype, rcode = %entry.rcode, answers, source = %entry.source, "dns query");
        // Log hỏng không được làm hỏng câu trả lời.
        if let Err(e) = self.storage.log_dns_query(entry, QUERY_LOG_LIMIT).await {
            tracing::warn!("dns query log: {e}");
        }
        outcome
    }

    async fn answer(&self, name: &str, qtype: RecordType) -> Outcome {
        let storage = self.storage.as_ref();
        let (faults, zone) = match (
            storage.list_dns_faults().await,
            storage.list_dns_records().await,
        ) {
            (Ok(faults), Ok(zone)) => (faults, zone),
            (Err(e), _) | (_, Err(e)) => {
                tracing::error!("dns storage: {e}");
                return Outcome::Reply(failed(ResponseCode::ServFail, Source::None));
            }
        };
        if let Some(fault) = resolver::find_fault(&faults, name, qtype) {
            let code = match fault.action {
                FaultAction::Servfail => Some(ResponseCode::ServFail),
                FaultAction::Refused => Some(ResponseCode::Refused),
                FaultAction::Nxdomain => Some(ResponseCode::NXDomain),
                FaultAction::Timeout => return Outcome::Drop,
                FaultAction::Delay => {
                    tokio::time::sleep(Duration::from_millis(fault.delay_ms).min(MAX_DELAY)).await;
                    None
                }
            };
            if let Some(code) = code {
                return Outcome::Reply(failed(code, Source::Fault));
            }
        }
        let mut answer = resolver::resolve(&zone, name, qtype);
        if let (Some(upstream), Some(unowned)) = (&self.upstream, answer.unowned.take()) {
            // Giữ chuỗi CNAME đã đi trong mock, hỏi resolver thật phần còn lại.
            answer.source = Source::Upstream;
            match upstream.query(&unowned, qtype).await {
                Ok((code, records)) => {
                    answer.code = code;
                    answer.records.extend(records);
                }
                Err(e) => {
                    tracing::warn!("dns upstream {}: {e}", upstream.address());
                    answer.code = ResponseCode::ServFail;
                }
            }
        }
        Outcome::Reply(answer)
    }
}

/// Chạy UDP + TCP trên cùng cổng; trả về khi một trong hai dừng.
pub async fn run(host: &str, cfg: &DnsConfig, storage: Arc<dyn Storage>) -> Result<()> {
    if let Some(path) = cfg.seed_file.as_deref().filter(|p| !p.is_empty()) {
        let loaded = seed::load(storage.as_ref(), path)
            .await
            .map_err(std::io::Error::other)?;
        tracing::info!("dns seed {path}: {loaded} record");
    }
    let engine = Engine::new(storage).with_upstream(cfg.upstream.as_deref());
    if let Some(upstream) = engine.upstream() {
        tracing::info!("dns forwards unowned names to {upstream}");
    }
    let addr = format!("{host}:{}", cfg.port);
    let udp = UdpSocket::bind(&addr).await?;
    let tcp = TcpListener::bind(&addr).await?;
    tracing::info!("dns listening on {addr} (udp+tcp)");
    tokio::try_join!(
        server::serve_udp(udp, engine.clone()),
        server::serve_tcp(tcp, engine)
    )?;
    Ok(())
}
