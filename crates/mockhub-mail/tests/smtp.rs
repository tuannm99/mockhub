//! Một phiên SMTP thật qua TCP, rồi đọc lại từ storage.
use mockhub_mail::smtp::serve;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

#[tokio::test]
async fn a_mail_sent_over_smtp_is_stored_with_its_envelope() {
    let storage = mockhub_storage::connect("sqlite::memory:").await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve(listener, storage.clone(), 10));

    let mut stream = BufReader::new(TcpStream::connect(addr).await.unwrap());
    let mut line = String::new();
    stream.read_line(&mut line).await.unwrap();
    assert!(line.starts_with("220 "), "{line}");
    let steps: [(&str, &str); 7] = [
        ("EHLO app.test", "250"),
        ("AUTH PLAIN AGEAYg==", "235"),
        ("MAIL FROM:<app@mock.test>", "250"),
        ("RCPT TO:<bob@mock.test>", "250"),
        ("RCPT TO:<secret@mock.test>", "250"),
        ("DATA", "354"),
        (
            "From: app@mock.test\r\nTo: bob@mock.test\r\nSubject: hi\r\n\r\n..dot line\r\nbody\r\n.",
            "250",
        ),
    ];
    for (send, want) in steps {
        stream
            .write_all(format!("{send}\r\n").as_bytes())
            .await
            .unwrap();
        loop {
            line.clear();
            stream.read_line(&mut line).await.unwrap();
            assert!(line.starts_with(want), "{send:?} answered {line:?}");
            if line.as_bytes().get(3) != Some(&b'-') {
                break;
            }
        }
    }
    let mails = storage
        .list_mails(Some("secret@mock.test"), 10)
        .await
        .unwrap();
    assert_eq!(mails.len(), 1);
    let mail = &mails[0];
    assert_eq!(
        (mail.from.as_str(), mail.subject.as_str()),
        ("app@mock.test", "hi")
    );
    assert_eq!(mail.to, ["bob@mock.test"]);
    assert_eq!(mail.bcc, ["secret@mock.test"]);
    assert_eq!(mail.text.replace("\r\n", "\n").trim(), ".dot line\nbody");
    assert!(
        !mail.raw.contains("secret@mock.test"),
        "BCC is only in the envelope"
    );
}
