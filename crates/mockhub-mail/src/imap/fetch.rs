//! Dựng dữ liệu cho `FETCH` từ nguyên văn thư. Hàm thuần.
use mail_parser::{AddressList, MessageParser};
use mockhub_core::domain::mail::Mail;

use super::parse::section_of;
use crate::mailbox::split_message;

/// Literal `{n}`: đúng từng byte.
fn literal(out: &mut Vec<u8>, label: &str, data: &[u8]) {
    out.extend_from_slice(format!("{label} {{{}}}\r\n", data.len()).as_bytes());
    out.extend_from_slice(data);
}

fn quote(text: &str) -> String {
    let clean = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\r', '\n'], " ");
    format!("\"{clean}\"")
}

fn nstring(text: Option<&str>) -> String {
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => quote(t),
        None => "NIL".to_owned(),
    }
}

fn address_list(list: Option<AddressList<'_>>) -> String {
    let rows: String = list
        .iter()
        .flat_map(|l| l.mailboxes())
        .filter_map(|m| {
            let address = m.address()?;
            let (local, host) = address.split_once('@').unwrap_or((address, ""));
            Some(format!(
                "({} NIL {} {})",
                nstring(m.name()),
                nstring(Some(local)),
                nstring(Some(host))
            ))
        })
        .collect();
    if rows.is_empty() {
        "NIL".to_owned()
    } else {
        format!("({rows})")
    }
}

fn envelope(mail: &Mail, head: &[u8]) -> String {
    let Some(message) = MessageParser::default().parse(head) else {
        let subject = nstring(Some(&mail.subject));
        return format!("(NIL {subject} NIL NIL NIL NIL NIL NIL NIL NIL)");
    };
    // Header thô: client tự giải mã encoded-word, như với server thật.
    let raw = |name: &'static str| {
        let value = message
            .headers()
            .get(name)
            .map(|h| String::from_utf8_lossy(h.raw_value()).into_owned());
        nstring(value.as_deref())
    };
    let from = address_list(message.from());
    format!(
        "({} {} {from} {from} {from} {} {} NIL NIL {})",
        raw("Date"),
        raw("Subject"),
        address_list(message.to()),
        address_list(message.cc()),
        raw("Message-ID"),
    )
}

/// `2026-11-27T10:05:00.123Z` thành `27-Nov-2026 10:05:00 +0000`.
pub fn internal_date(rfc3339: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let part = |range| rfc3339.get(range).unwrap_or_default();
    let month = part(5..7)
        .parse::<usize>()
        .ok()
        .and_then(|m| MONTHS.get(m.checked_sub(1)?))
        .unwrap_or(&"Jan");
    format!(
        "{}-{month}-{} {} +0000",
        part(8..10),
        part(0..4),
        part(11..19)
    )
}

/// Chỉ giữ các field được hỏi (kèm dòng nối tiếp của chúng).
fn header_fields(head: &[u8], wanted: &[String], negate: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut keep = false;
    for line in head.split_inclusive(|b| *b == b'\n') {
        if line == b"\r\n" || line == b"\n" {
            continue;
        }
        if !matches!(line.first(), Some(b' ' | b'\t')) {
            let name = line.split(|b| *b == b':').next().unwrap_or_default();
            let listed = wanted
                .iter()
                .any(|w| w.as_bytes().eq_ignore_ascii_case(name.trim_ascii()));
            keep = listed != negate;
        }
        if keep {
            out.extend_from_slice(line);
        }
    }
    out.extend_from_slice(b"\r\n");
    out
}

fn section_data(section: &str, raw: &[u8]) -> Vec<u8> {
    let (head, body) = split_message(raw);
    let upper = section.to_ascii_uppercase();
    match upper.as_str() {
        "" => raw.to_vec(),
        "HEADER" => head,
        "TEXT" | "1" => body.to_vec(),
        _ if upper.starts_with("HEADER.FIELDS") => {
            let (Some(open), Some(close)) = (section.find('('), section.rfind(')')) else {
                return head;
            };
            let wanted: Vec<String> = section
                .get(open + 1..close)
                .unwrap_or_default()
                .split_whitespace()
                .map(|w| w.trim_matches('"').to_owned())
                .collect();
            header_fields(&head, &wanted, upper.starts_with("HEADER.FIELDS.NOT"))
        }
        _ => Vec::new(),
    }
}

/// Mục này có cần nội dung thư không (`UID`, `FLAGS`, `RFC822.SIZE` thì không).
pub fn needs_content(item: &str) -> bool {
    !matches!(
        item.to_ascii_uppercase().as_str(),
        "UID" | "FLAGS" | "RFC822.SIZE"
    )
}

/// Đọc mục này có đánh dấu đã đọc không (`.PEEK` và header thì không).
pub fn marks_seen(item: &str) -> bool {
    let upper = item.to_ascii_uppercase();
    upper == "RFC822" || upper == "RFC822.TEXT" || upper.starts_with("BODY[")
}

/// Thêm một mục cần nội dung vào `out`. Trả `false` nếu không biết mục này.
pub fn content_item(out: &mut Vec<u8>, item: &str, mail: &Mail, raw: &[u8]) -> bool {
    let upper = item.to_ascii_uppercase();
    let (head, body) = split_message(raw);
    match upper.as_str() {
        "INTERNALDATE" => out.extend_from_slice(
            format!("INTERNALDATE \"{}\"", internal_date(&mail.received_at)).as_bytes(),
        ),
        "ENVELOPE" => {
            out.extend_from_slice(format!("ENVELOPE {}", envelope(mail, &head)).as_bytes())
        }
        "BODY" | "BODYSTRUCTURE" => {
            let lines = body.iter().filter(|b| **b == b'\n').count();
            out.extend_from_slice(
                format!(
                    "{upper} (\"TEXT\" \"PLAIN\" (\"CHARSET\" \"UTF-8\") NIL NIL \"8BIT\" {} {lines})",
                    body.len()
                )
                .as_bytes(),
            );
        }
        "RFC822" => literal(out, "RFC822", raw),
        "RFC822.HEADER" => literal(out, "RFC822.HEADER", &head),
        "RFC822.TEXT" => literal(out, "RFC822.TEXT", body),
        _ if upper.starts_with("BODY[") || upper.starts_with("BODY.PEEK[") => {
            let (section, partial) = section_of(item);
            let mut data = section_data(&section, raw);
            let mut label = format!("BODY[{section}]");
            if let Some((from, count)) = partial {
                data.drain(..from.min(data.len()));
                if let Some(count) = count {
                    data.truncate(count);
                }
                label = format!("{label}<{from}>");
            }
            literal(out, &label, &data);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_use_the_imap_form() {
        assert_eq!(
            internal_date("2026-11-27T10:05:09.123Z"),
            "27-Nov-2026 10:05:09 +0000"
        );
        internal_date("garbage");
    }

    #[test]
    fn header_fields_keep_continuations() {
        let raw = b"From: a@x\r\nSubject: one\r\n two\r\nTo: b@x\r\n\r\nbody";
        assert_eq!(
            section_data("HEADER.FIELDS (subject)", raw),
            b"Subject: one\r\n two\r\n\r\n"
        );
        assert_eq!(
            section_data("HEADER.FIELDS.NOT (Subject From)", raw),
            b"To: b@x\r\n\r\n"
        );
        assert_eq!(section_data("TEXT", raw), b"body");
    }
}
