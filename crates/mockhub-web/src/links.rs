//! Tìm link trong thư (lời mời, đặt lại mật khẩu) để UI cho sao chép / mở.

/// Chỉ `http://` và `https://`, theo thứ tự xuất hiện, không lặp. `&amp;` của HTML được đổi lại.
pub fn find(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("http") {
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || "<>\"')".contains(c))
            .unwrap_or(tail.len());
        let url = tail[..end]
            .trim_end_matches(['.', ',', ';'])
            .replace("&amp;", "&");
        let scheme_ok = ["http://", "https://"]
            .iter()
            .any(|s| url.starts_with(s) && url.len() > s.len());
        if scheme_ok && !found.contains(&url) {
            found.push(url);
        }
        rest = &tail[end.max(4)..];
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_links_once_without_trailing_punctuation() {
        assert_eq!(
            find(
                "Mở http://x/a?t=1&amp;u=2. Hoặc <a href=\"https://y/b\">https://y/b</a> httpx http:// é"
            ),
            ["http://x/a?t=1&u=2", "https://y/b"]
        );
        assert!(find("javascript:alert(1)").is_empty());
    }
}
