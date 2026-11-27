//! Đổi một thư vừa nhận (phong bì + nguyên văn) thành `NewMail`.
use mail_parser::MessageParser;
use mockhub_core::domain::mail::NewMail;

/// Chia người nhận trong phong bì thành to / cc / bcc theo header. BCC không có trong header, nên là
/// những địa chỉ `RCPT TO` không thấy ở `To` lẫn `Cc`.
pub fn classify(
    recipients: &[String],
    header_to: &[String],
    header_cc: &[String],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (mut to, mut cc, mut bcc) = (Vec::new(), Vec::new(), Vec::new());
    let listed =
        |list: &[String], address: &str| list.iter().any(|a| a.eq_ignore_ascii_case(address));
    let mut seen: Vec<String> = Vec::new();
    for address in recipients {
        if listed(&seen, address) {
            continue;
        }
        seen.push(address.clone());
        let bucket = if listed(header_to, address) {
            &mut to
        } else if listed(header_cc, address) {
            &mut cc
        } else {
            &mut bcc
        };
        bucket.push(address.clone());
    }
    (to, cc, bcc)
}

/// Không bao giờ làm mất thư: không parse được thì vẫn giữ nguyên văn, mọi người nhận tính là `to`.
pub fn to_new_mail(from: String, recipients: &[String], raw: Vec<u8>) -> NewMail {
    let Some(message) = MessageParser::default().parse(&raw) else {
        let text = String::from_utf8_lossy(&raw).into_owned();
        return NewMail {
            from,
            to: recipients.to_vec(),
            text,
            raw,
            ..Default::default()
        };
    };
    let addresses = |list: Option<mail_parser::AddressList<'_>>| -> Vec<String> {
        list.map(|l| {
            l.mailboxes()
                .filter_map(|m| m.address().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
    };
    let (to, cc, bcc) = classify(
        recipients,
        &addresses(message.to()),
        &addresses(message.cc()),
    );
    let subject = message.subject().unwrap_or_default().to_owned();
    let text = message
        .body_text(0)
        .map(|t| t.into_owned())
        .unwrap_or_default();
    // `body_html` dựng HTML từ phần text khi thư không có HTML; chỉ giữ HTML thật.
    let html = message
        .html_body()
        .next()
        .filter(|part| part.is_content_type("text", "html"))
        .and_then(|_| message.body_html(0))
        .map(|h| h.into_owned());
    NewMail {
        from,
        to,
        cc,
        bcc,
        subject,
        text,
        html,
        raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn bcc_is_what_the_headers_do_not_name() {
        let (to, cc, bcc) = classify(
            &owned(&["A@mock.test", "b@mock.test", "c@mock.test", "a@mock.test"]),
            &owned(&["a@mock.test"]),
            &owned(&["B@mock.test"]),
        );
        assert_eq!(
            (to, cc, bcc),
            (
                owned(&["A@mock.test"]),
                owned(&["b@mock.test"]),
                owned(&["c@mock.test"])
            )
        );
    }

    #[test]
    fn encoded_headers_and_bodies_are_decoded() {
        let raw = b"From: App <app@mock.test>\r\nTo: Bob <bob@mock.test>\r\n\
Subject: =?utf-8?B?TOG7nWkgbeG7nWk=?=\r\nContent-Type: text/plain; charset=utf-8\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\r\nhttp://portal.test/accept-invite?token=3Dabc=\r\ndef\r\n"
            .to_vec();
        let mail = to_new_mail("app@mock.test".into(), &owned(&["bob@mock.test"]), raw);
        assert_eq!(mail.subject, "Lời mời");
        assert_eq!(
            mail.text.trim(),
            "http://portal.test/accept-invite?token=abcdef"
        );
        assert_eq!(mail.to, owned(&["bob@mock.test"]));
        assert_eq!(mail.html, None, "a text mail has no HTML of its own");
    }

    #[test]
    fn a_multipart_mail_keeps_both_bodies() {
        let raw = b"To: bob@mock.test\r\nSubject: hi\r\nContent-Type: multipart/alternative; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nplain\r\n--b\r\nContent-Type: text/html\r\n\r\n<p>rich</p>\r\n--b--\r\n"
            .to_vec();
        let mail = to_new_mail("app@mock.test".into(), &owned(&["bob@mock.test"]), raw);
        assert_eq!(mail.text.trim(), "plain");
        assert_eq!(mail.html.as_deref().map(str::trim), Some("<p>rich</p>"));
    }

    #[test]
    fn something_that_is_not_a_mail_is_still_kept() {
        let mail = to_new_mail("a@mock.test".into(), &owned(&["b@mock.test"]), Vec::new());
        assert_eq!(mail.to, owned(&["b@mock.test"]));
    }
}
