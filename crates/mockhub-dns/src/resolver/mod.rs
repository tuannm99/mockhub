//! Quyết định câu trả lời. Hàm thuần: không socket, không DB.
use hickory_proto::{
    op::ResponseCode,
    rr::{RData, Record, RecordType},
};
use mockhub_core::domain::dns::{DnsFault, DnsRecord};

use crate::records::{parse_kind, rdata, wire_name};

const MAX_CNAME_HOPS: usize = 8;

#[derive(Debug, Clone)]
pub struct Answer {
    pub code: ResponseCode,
    pub records: Vec<Record>,
    pub source: Source,
    /// Tên mà mock không sở hữu (tên được hỏi, hoặc đích của chuỗi CNAME): chỗ để forward.
    pub unowned: Option<String>,
}

/// Câu trả lời đến từ đâu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Mock,
    Upstream,
    Fault,
    /// Không record nào liên quan.
    None,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Upstream => "upstream",
            Self::Fault => "fault",
            Self::None => "none",
        }
    }
}

fn from_mock(code: ResponseCode, records: Vec<Record>) -> Answer {
    Answer {
        code,
        records,
        source: Source::Mock,
        unowned: None,
    }
}

/// Các wildcard có thể phủ `name`, gần nhất trước: `a.b.test` → `*.b.test`, `*.test`.
fn wildcards(name: &str) -> impl Iterator<Item = String> + '_ {
    name.match_indices('.')
        .map(|(i, _)| format!("*{}", &name[i..]))
}

/// Tên có record, hoặc có gì đó nằm dưới nó (empty non-terminal): là NODATA chứ không phải NXDOMAIN.
fn exists(records: &[DnsRecord], name: &str) -> bool {
    records.iter().any(|r| {
        r.name == name
            || r.name
                .strip_suffix(name)
                .is_some_and(|head| head.ends_with('.'))
    })
}

/// Record của `name`: của chính nó, hoặc của wildcard phủ nó khi bản thân tên không tồn tại (RFC 4592).
fn records_at<'a>(records: &'a [DnsRecord], name: &str) -> Option<Vec<&'a DnsRecord>> {
    if exists(records, name) {
        return Some(records.iter().filter(|r| r.name == name).collect());
    }
    wildcards(name)
        .map(|w| records.iter().filter(|r| r.name == w).collect::<Vec<_>>())
        .find(|found| !found.is_empty())
}

/// Fault áp cho (`name`, `kind`): khớp đúng tên trước, rồi wildcard gần nhất.
pub fn find_fault<'a>(
    faults: &'a [DnsFault],
    name: &str,
    kind: RecordType,
) -> Option<&'a DnsFault> {
    let kind = kind.to_string();
    std::iter::once(name.to_owned())
        .chain(wildcards(name))
        .find_map(|candidate| {
            faults
                .iter()
                .find(|f| f.name == candidate && f.kind.as_ref().is_none_or(|k| *k == kind))
        })
}

fn wire(owner: &str, r: &DnsRecord) -> Option<Record> {
    let data = rdata(parse_kind(&r.kind).ok()?, &r.value).ok()?;
    Some(Record::from_rdata(wire_name(owner).ok()?, r.ttl, data))
}

/// Trả lời một câu hỏi như một resolver: đi theo chuỗi CNAME, wildcard, NODATA, NXDOMAIN.
/// `name` đã chuẩn hoá (chữ thường, không dấu chấm cuối).
pub fn resolve(records: &[DnsRecord], name: &str, qtype: RecordType) -> Answer {
    let mut chain = Vec::new();
    let mut seen = vec![name.to_owned()];
    let mut current = name.to_owned();
    loop {
        let Some(here) = records_at(records, &current) else {
            // Cả CNAME trỏ vào chỗ trống: trả chuỗi đã đi, kèm NXDOMAIN cho đích.
            let source = if chain.is_empty() {
                Source::None
            } else {
                Source::Mock
            };
            return Answer {
                code: ResponseCode::NXDomain,
                records: chain,
                source,
                unowned: Some(current),
            };
        };
        let follow = qtype != RecordType::CNAME && qtype != RecordType::ANY;
        let alias = here
            .iter()
            .find(|r| follow && r.kind == "CNAME")
            .and_then(|r| wire(&current, r));
        if let Some(cname) = alias {
            let RData::CNAME(target) = &cname.data else {
                return from_mock(ResponseCode::ServFail, chain);
            };
            let target = target
                .0
                .to_ascii()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            chain.push(cname);
            if seen.contains(&target) || seen.len() > MAX_CNAME_HOPS {
                return from_mock(ResponseCode::ServFail, chain);
            }
            seen.push(target.clone());
            current = target;
            continue;
        }
        chain.extend(
            here.iter()
                .filter(|r| qtype == RecordType::ANY || r.kind == qtype.to_string())
                .filter_map(|r| wire(&current, r)),
        );
        return from_mock(ResponseCode::NoError, chain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockhub_core::domain::dns::FaultAction;

    fn zone(entries: &[(&str, &str, &str)]) -> Vec<DnsRecord> {
        entries
            .iter()
            .enumerate()
            .map(|(i, (name, kind, value))| DnsRecord {
                id: i as i64,
                name: (*name).into(),
                kind: (*kind).into(),
                value: (*value).into(),
                ttl: 30,
            })
            .collect()
    }

    fn ask(z: &[DnsRecord], name: &str, qtype: RecordType) -> (ResponseCode, Vec<String>) {
        let a = resolve(z, name, qtype);
        (
            a.code,
            a.records
                .iter()
                .map(|r| format!("{} {} {}", r.name, r.record_type(), r.data))
                .collect(),
        )
    }

    #[test]
    fn nodata_and_nxdomain_differ() {
        let z = zone(&[("_waf-verify.shop.test", "TXT", "token")]);
        assert_eq!(
            ask(&z, "_waf-verify.shop.test", RecordType::A),
            (ResponseCode::NoError, vec![])
        );
        assert_eq!(
            ask(&z, "shop.test", RecordType::TXT),
            (ResponseCode::NoError, vec![]),
            "empty non-terminal"
        );
        assert_eq!(
            ask(&z, "other.test", RecordType::TXT).0,
            ResponseCode::NXDomain
        );
        assert_eq!(
            ask(&z, "op.test", RecordType::TXT).0,
            ResponseCode::NXDomain,
            "a suffix is not a parent"
        );
    }

    #[test]
    fn a_cname_chain_is_followed_and_returned_whole() {
        let z = zone(&[
            ("www.shop.test", "CNAME", "z1.edge.waf.local"),
            ("*.edge.waf.local", "A", "127.0.0.1"),
        ]);
        let (code, got) = ask(&z, "www.shop.test", RecordType::A);
        assert_eq!(code, ResponseCode::NoError);
        assert_eq!(
            got,
            [
                "www.shop.test. CNAME z1.edge.waf.local.",
                "z1.edge.waf.local. A 127.0.0.1"
            ]
        );
        assert_eq!(
            ask(&z, "www.shop.test", RecordType::CNAME).1.len(),
            1,
            "asking for CNAME does not follow it"
        );
    }

    #[test]
    fn a_dangling_or_looping_cname_fails_as_a_resolver_does() {
        let dangling = zone(&[("a.test", "CNAME", "gone.test")]);
        let (code, got) = ask(&dangling, "a.test", RecordType::A);
        assert_eq!((code, got.len()), (ResponseCode::NXDomain, 1));
        let looping = zone(&[("a.test", "CNAME", "b.test"), ("b.test", "CNAME", "a.test")]);
        assert_eq!(
            ask(&looping, "a.test", RecordType::A).0,
            ResponseCode::ServFail
        );
    }

    #[test]
    fn a_wildcard_covers_only_names_that_do_not_exist() {
        let z = zone(&[
            ("*.app.test", "A", "127.0.0.2"),
            ("real.app.test", "TXT", "x"),
        ]);
        assert_eq!(
            ask(&z, "b.a.app.test", RecordType::A).1,
            ["b.a.app.test. A 127.0.0.2"]
        );
        assert_eq!(
            ask(&z, "real.app.test", RecordType::A),
            (ResponseCode::NoError, vec![])
        );
        assert_eq!(
            ask(&z, "app.test", RecordType::A),
            (ResponseCode::NoError, vec![]),
            "the wildcard's parent exists but owns nothing"
        );
    }

    #[test]
    fn the_exact_fault_wins_over_a_wildcard_and_respects_the_kind() {
        let fault = |name: &str, kind: Option<&str>, action| DnsFault {
            id: 0,
            name: name.into(),
            kind: kind.map(Into::into),
            action,
            delay_ms: 0,
        };
        let faults = [
            fault("*.test", None, FaultAction::Servfail),
            fault("a.test", Some("TXT"), FaultAction::Timeout),
        ];
        assert_eq!(
            find_fault(&faults, "a.test", RecordType::TXT)
                .unwrap()
                .action,
            FaultAction::Timeout
        );
        assert_eq!(
            find_fault(&faults, "a.test", RecordType::A).unwrap().action,
            FaultAction::Servfail
        );
        assert!(find_fault(&faults, "test", RecordType::A).is_none());
    }
}
