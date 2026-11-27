//! So khớp admin token (dùng chung cho REST, web, IMAP, POP3).
use base64::{Engine as _, engine::general_purpose::STANDARD};
use subtle::ConstantTimeEq;

/// So sánh không lộ vị trí byte sai qua thời gian.
pub fn secret_matches(expected: &str, given: &str) -> bool {
    expected.as_bytes().ct_eq(given.as_bytes()).into()
}

/// Giá trị header `Authorization` có đúng `Bearer <token>` không.
pub fn bearer_matches(token: &str, header: &str) -> bool {
    header.split_once(' ').is_some_and(|(scheme, given)| {
        scheme.eq_ignore_ascii_case("bearer") && secret_matches(token, given.trim())
    })
}

/// HTTP Basic với tên bất kỳ và mật khẩu là token.
pub fn basic_matches(token: &str, header: &str) -> bool {
    let Some((scheme, encoded)) = header.split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("basic") {
        return false;
    }
    STANDARD
        .decode(encoded.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|pair| {
            pair.split_once(':')
                .map(|(_, password)| secret_matches(token, password))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_token_matches() {
        assert!(bearer_matches("s3cret", "Bearer s3cret"));
        for bad in [
            "Bearer s3cre",
            "Bearer s3cret2",
            "s3cret",
            "Basic s3cret",
            "",
        ] {
            assert!(!bearer_matches("s3cret", bad), "{bad:?}");
        }
        // "any:s3cret" và "any:wrong"
        assert!(basic_matches("s3cret", "Basic YW55OnMzY3JldA=="));
        assert!(!basic_matches("s3cret", "Basic YW55Ondyb25n"));
        assert!(!basic_matches("s3cret", "Basic not-base64!"));
    }
}
