//! Chuẩn hoá và kiểm tra record/fault lúc ghi, và đổi record đã lưu sang `RData` của DNS.
use std::str::FromStr;

use hickory_proto::rr::{Name, RData, RecordType, rdata::TXT};
use mockhub_core::domain::dns::{DnsRecord, NewDnsFault, NewDnsRecord};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RecordError {
    #[error("tên không hợp lệ: {0:?}")]
    Name(String),
    #[error("loại record không hỗ trợ: {0:?}")]
    Kind(String),
    #[error("giá trị {kind} không hợp lệ: {value:?}")]
    Value { kind: String, value: String },
    #[error("{0} đã có CNAME hoặc record khác: CNAME không được nằm chung tên")]
    CnameConflict(String),
}

/// Chữ thường, bỏ dấu chấm cuối. `_` và wildcard `*` ở nhãn đầu được chấp nhận.
pub fn normalize_name(name: &str) -> Result<String, RecordError> {
    let name = name.trim().trim_end_matches('.').to_ascii_lowercase();
    let bad = || RecordError::Name(name.clone());
    if name.is_empty() || name.len() > 253 {
        return Err(bad());
    }
    for (i, label) in name.split('.').enumerate() {
        let wildcard = label == "*" && i == 0;
        let plain = !label.is_empty()
            && label.len() <= 63
            && label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !wildcard && !plain {
            return Err(bad());
        }
    }
    Ok(name)
}

pub fn parse_kind(kind: &str) -> Result<RecordType, RecordError> {
    let upper = kind.trim().to_ascii_uppercase();
    RecordType::from_str(&upper).map_err(|_| RecordError::Kind(kind.to_owned()))
}

/// `RData` của một giá trị viết theo cú pháp zone file. TXT là chuỗi thô, tự tách đoạn 255 byte.
pub fn rdata(kind: RecordType, value: &str) -> Result<RData, RecordError> {
    let bad = || RecordError::Value {
        kind: kind.to_string(),
        value: value.to_owned(),
    };
    if kind == RecordType::TXT {
        let chunks: Vec<&[u8]> = if value.is_empty() {
            vec![b""]
        } else {
            value.as_bytes().chunks(255).collect()
        };
        return TXT::try_new(chunks.into_iter())
            .map(RData::TXT)
            .map_err(|_| bad());
    }
    let mut value = value.trim().to_owned();
    // Tên đích luôn là tên đầy đủ (không có zone origin để nối vào).
    if matches!(
        kind,
        RecordType::CNAME | RecordType::NS | RecordType::PTR | RecordType::MX | RecordType::SRV
    ) && !value.ends_with('.')
    {
        value.push('.');
    }
    RData::try_from_str(kind, &value).map_err(|_| bad())
}

pub fn wire_name(name: &str) -> Result<Name, RecordError> {
    let mut parsed = Name::from_ascii(name).map_err(|_| RecordError::Name(name.to_owned()))?;
    parsed.set_fqdn(true);
    Ok(parsed)
}

/// Chuẩn hoá record sắp thêm và từ chối thứ một zone thật sẽ từ chối. `replacing`: các record cùng
/// tên + loại sẽ bị thay, nên không tính là xung đột.
pub fn prepare(
    new: NewDnsRecord,
    existing: &[DnsRecord],
    replacing: bool,
) -> Result<NewDnsRecord, RecordError> {
    let name = normalize_name(&new.name)?;
    let kind = parse_kind(&new.kind)?;
    rdata(kind, &new.value)?;
    wire_name(&name)?;
    let kind = kind.to_string();
    let conflict = existing
        .iter()
        .filter(|r| r.name == name && !(replacing && r.kind == kind))
        .any(|r| r.kind == "CNAME" || kind == "CNAME");
    if conflict {
        return Err(RecordError::CnameConflict(name));
    }
    Ok(NewDnsRecord { name, kind, ..new })
}

pub fn prepare_fault(new: NewDnsFault) -> Result<NewDnsFault, RecordError> {
    let name = normalize_name(&new.name)?;
    let kind = match new.kind.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(k) => Some(parse_kind(k)?.to_string()),
    };
    Ok(NewDnsFault { name, kind, ..new })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockhub_core::domain::dns::FaultAction;

    fn new(name: &str, kind: &str, value: &str) -> NewDnsRecord {
        NewDnsRecord {
            name: name.into(),
            kind: kind.into(),
            value: value.into(),
            ttl: None,
        }
    }

    #[test]
    fn names_are_normalised_and_checked() {
        assert_eq!(
            normalize_name(" _WAF-Verify.Shop.test. ").unwrap(),
            "_waf-verify.shop.test"
        );
        assert_eq!(
            normalize_name("*.edge.waf.local").unwrap(),
            "*.edge.waf.local"
        );
        for bad in ["", ".", "a..b", "a.*.b", "a b.test", &"x".repeat(64)] {
            assert!(normalize_name(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn values_are_checked_per_kind() {
        for (kind, value) in [
            ("a", "127.0.0.1"),
            ("AAAA", "::1"),
            ("CNAME", "z1.edge.waf.local"),
            ("TXT", "v=spf1 -all"),
            ("MX", "10 mail.mock.test"),
            ("CAA", "0 issue \"ca.mock.test\""),
        ] {
            prepare(new("app.test", kind, value), &[], false)
                .unwrap_or_else(|e| panic!("{kind}: {e}"));
        }
        assert!(matches!(
            prepare(new("app.test", "A", "nope"), &[], false),
            Err(RecordError::Value { .. })
        ));
        assert!(matches!(
            prepare(new("app.test", "BOGUS", "x"), &[], false),
            Err(RecordError::Kind(_))
        ));
    }

    #[test]
    fn a_long_txt_is_split_into_strings_of_255_bytes() {
        let RData::TXT(txt) = rdata(RecordType::TXT, &"a".repeat(600)).unwrap() else {
            panic!()
        };
        assert_eq!(
            txt.txt_data.iter().map(|c| c.len()).collect::<Vec<_>>(),
            [255, 255, 90]
        );
    }

    #[test]
    fn a_cname_does_not_share_its_name() {
        let have = vec![DnsRecord {
            id: 1,
            name: "www.app.test".into(),
            kind: "A".into(),
            value: "127.0.0.1".into(),
            ttl: 60,
        }];
        assert!(prepare(new("www.app.test", "CNAME", "x.test"), &have, false).is_err());
        assert!(prepare(new("www.app.test", "TXT", "x"), &have, false).is_ok());
        let cname = vec![DnsRecord {
            kind: "CNAME".into(),
            value: "x.test".into(),
            ..have[0].clone()
        }];
        assert!(prepare(new("www.app.test", "CNAME", "y.test"), &cname, false).is_err());
        assert!(prepare(new("www.app.test", "CNAME", "y.test"), &cname, true).is_ok());
    }

    #[test]
    fn a_fault_keeps_no_kind_when_none_is_given() {
        let f = prepare_fault(NewDnsFault {
            name: "*.Flaky.test".into(),
            kind: Some("".into()),
            action: FaultAction::Timeout,
            delay_ms: 0,
        })
        .unwrap();
        assert_eq!((f.name.as_str(), f.kind), ("*.flaky.test", None));
    }
}
