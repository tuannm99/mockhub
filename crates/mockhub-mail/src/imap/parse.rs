//! Tách tham số lệnh IMAP. Hàm thuần.

/// Tách theo dấu cách, giữ nguyên chuỗi trong ngoặc kép, danh sách `(...)` và đoạn `[...]`.
pub fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let (mut depth, mut quoted) = (0u32, false);
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if quoted {
            current.push(c);
            if c == '\\' {
                current.extend(chars.next());
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            ' ' if depth == 0 => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub fn unquote(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return s.to_owned();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.extend(chars.next());
        } else {
            out.push(c);
        }
    }
    out
}

/// Chỉ số (tăng dần, không lặp) của các phần tử mà khoá nằm trong tập `1:3,5,7:*`.
/// `keys` là số thứ tự hoặc UID của từng thư; `*` là khoá lớn nhất.
pub fn resolve_set(set: &str, keys: &[i64]) -> Vec<usize> {
    let largest = keys.iter().copied().max().unwrap_or(0);
    let number = |s: &str| {
        if s == "*" {
            largest
        } else {
            s.parse().unwrap_or(0)
        }
    };
    let ranges: Vec<(i64, i64)> = set
        .split(',')
        .map(|part| {
            let (low, high) = part.split_once(':').unwrap_or((part, part));
            let (a, b) = (number(low), number(high));
            (a.min(b), a.max(b))
        })
        .collect();
    (0..keys.len())
        .filter(|i| ranges.iter().any(|(a, b)| (*a..=*b).contains(&keys[*i])))
        .collect()
}

/// Mở các macro `ALL`/`FAST`/`FULL` và tách `(FLAGS UID BODY[HEADER.FIELDS (From To)])`.
pub fn fetch_items(rest: &str) -> Vec<String> {
    let rest = rest.trim();
    let basic = ["FLAGS", "INTERNALDATE", "RFC822.SIZE"];
    let extra: &[&str] = match rest.to_ascii_uppercase().as_str() {
        "ALL" => &["ENVELOPE"],
        "FAST" => &[],
        "FULL" => &["ENVELOPE", "BODY"],
        _ => {
            let inner = rest
                .strip_prefix('(')
                .and_then(|r| r.strip_suffix(')'))
                .unwrap_or(rest);
            return tokens(inner);
        }
    };
    basic.iter().chain(extra).map(|s| (*s).to_owned()).collect()
}

/// `BODY.PEEK[HEADER.FIELDS (From To)]<0.100>` thành đoạn (`HEADER.FIELDS (From To)`) và phần
/// cắt (`0.100`).
pub fn section_of(item: &str) -> (String, Option<(usize, Option<usize>)>) {
    let (Some(open), Some(close)) = (item.find('['), item.rfind(']')) else {
        return (String::new(), None);
    };
    if close < open {
        return (String::new(), None);
    }
    let section = item[open + 1..close].trim().to_owned();
    let partial = item[close + 1..]
        .strip_prefix('<')
        .and_then(|t| t.strip_suffix('>'))
        .and_then(|spec| {
            let (from, count) = spec.split_once('.').unwrap_or((spec, ""));
            Some((from.parse().ok()?, count.parse().ok()))
        });
    (section, partial)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_keep_their_groups() {
        assert_eq!(
            tokens(r#"1 (UID BODY[HEADER.FIELDS (From To)]) "a \"b\" c""#),
            ["1", "(UID BODY[HEADER.FIELDS (From To)])", r#""a \"b\" c""#]
        );
        assert_eq!(unquote(r#""a \"b\" c""#), r#"a "b" c"#);
        assert_eq!(unquote("\""), "\"");
    }

    #[test]
    fn sets_cover_ranges_in_either_order() {
        let uids = [3, 7, 9];
        assert_eq!(resolve_set("7", &uids), [1]);
        assert_eq!(resolve_set("1:7", &uids), [0, 1]);
        assert_eq!(resolve_set("*:7", &uids), [1, 2]);
        assert_eq!(resolve_set("100:*", &uids), [2], "RFC 3501: * is the last");
        assert_eq!(resolve_set("3,9,3", &uids), [0, 2]);
        assert!(resolve_set("x", &uids).is_empty());
        assert!(resolve_set("1:*", &[]).is_empty());
    }

    #[test]
    fn sections_and_partials() {
        assert_eq!(
            section_of("BODY.PEEK[HEADER.FIELDS (From)]<0.100>"),
            ("HEADER.FIELDS (From)".to_owned(), Some((0, Some(100))))
        );
        assert_eq!(section_of("BODY[]"), (String::new(), None));
        assert_eq!(section_of("BODY]["), (String::new(), None));
    }
}
