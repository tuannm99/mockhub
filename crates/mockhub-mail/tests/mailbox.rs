//! Phiên IMAP và POP3 thật qua TCP, trên cùng một kho thư.
use std::{sync::Arc, time::Duration};

use mockhub_core::domain::mail::NewMail;
use mockhub_mail::{imap, mailbox::Access, mime, pop3};
use mockhub_storage::Storage;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

fn mail(to: &str, subject: &str, body: &str) -> NewMail {
    let raw = format!(
        "From: Portal <no-reply@waf.local>\r\nTo: {to}\r\nSubject: {subject}\r\n\
Date: Sun, 22 Nov 2026 10:00:00 +0000\r\n\r\n{body}\r\n"
    );
    mime::to_new_mail(
        "no-reply@waf.local".into(),
        &[to.to_owned()],
        raw.into_bytes(),
    )
}

async fn seeded() -> Arc<dyn Storage> {
    let storage = mockhub_storage::connect("sqlite::memory:").await.unwrap();
    for new in [
        mail(
            "bob@acme.io",
            "Invite",
            "Join: http://x/accept?token=1\r\n.dot line",
        ),
        mail("eve@acme.io", "Reset", "Reset: http://x/reset?token=2"),
        mail("bob@acme.io", "Second", "Hello again"),
    ] {
        storage.save_mail(new, 50).await.unwrap();
    }
    storage
}

struct Client(BufReader<TcpStream>);

impl Client {
    async fn line(&mut self) -> String {
        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(5), self.0.read_line(&mut line));
        assert!(read.await.expect("no answer").unwrap() > 0, "closed");
        line.trim_end_matches(['\r', '\n']).to_owned()
    }

    async fn send(&mut self, text: &str) {
        self.0
            .write_all(format!("{text}\r\n").as_bytes())
            .await
            .unwrap();
    }

    async fn ask(&mut self, text: &str) -> String {
        self.send(text).await;
        self.line().await
    }

    /// Mọi dòng tới (gồm cả) dòng thoả `stop`.
    async fn until(&mut self, stop: impl Fn(&str) -> bool) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let line = self.line().await;
            let last = stop(&line);
            lines.push(line);
            if last {
                return lines;
            }
        }
    }

    /// Gửi lệnh IMAP, trả mọi dòng tới dòng mang tag của nó.
    async fn imap(&mut self, command: &str) -> Vec<String> {
        let tag = format!("{} ", command.split(' ').next().unwrap());
        self.send(command).await;
        self.until(|l| l.starts_with(&tag)).await
    }
}

async fn start(storage: Arc<dyn Storage>, protocol: &str, token: Option<&str>) -> Client {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let access = Access::new(token);
    if protocol == "imap" {
        tokio::spawn(imap::serve(listener, storage, access));
    } else {
        tokio::spawn(pop3::serve(listener, storage, access));
    }
    let mut client = Client(BufReader::new(TcpStream::connect(addr).await.unwrap()));
    client.line().await; // greeting
    client
}

#[tokio::test]
async fn pop3_reads_deletes_and_hides_on_quit() {
    let storage = seeded().await;
    let mut c = start(storage.clone(), "pop3", None).await;
    assert!(c.ask("STAT").await.starts_with("-ERR"));
    c.ask("USER bob@acme.io").await;
    assert_eq!(c.ask("PASS anything").await, "+OK 2 messages");
    assert!(c.ask("STAT").await.starts_with("+OK 2 "));
    c.send("UIDL").await;
    assert_eq!(c.until(|l| l == ".").await, ["+OK", "1 mh1", "2 mh3", "."]);
    c.send("RETR 1").await;
    let got = c.until(|l| l == ".").await.join("\n");
    assert!(
        got.contains("Subject: Invite") && got.contains("\n..dot line"),
        "{got}"
    );
    c.send("TOP 1 0").await;
    let top = c.until(|l| l == ".").await.join("\n");
    assert!(
        top.contains("Subject: Invite") && !top.contains("Join:"),
        "{top}"
    );
    assert!(c.ask("DELE 1").await.starts_with("+OK"));
    assert!(c.ask("RETR 1").await.starts_with("-ERR"));
    assert!(c.ask("RETR 0").await.starts_with("-ERR"));
    assert!(c.ask("QUIT").await.starts_with("+OK"));

    assert_eq!(storage.list_mailbox("bob@acme.io").await.unwrap().len(), 1);
    assert_eq!(storage.list_mailbox("eve@acme.io").await.unwrap().len(), 1);
    assert_eq!(storage.list_mails(None, 10).await.unwrap().len(), 3);
}

#[tokio::test]
async fn imap_selects_fetches_searches_stores_and_expunges() {
    let storage = seeded().await;
    let mut c = start(storage.clone(), "imap", None).await;
    c.imap("a1 LOGIN bob@acme.io pw").await;
    let select = c.imap("a2 SELECT INBOX").await.join("\n");
    for want in [
        "* 2 EXISTS",
        "[UIDVALIDITY ",
        "[UIDNEXT 4]",
        "a2 OK [READ-WRITE]",
    ] {
        assert!(select.contains(want), "missing {want}: {select}");
    }
    assert_eq!(c.imap("a3 SEARCH UNSEEN").await[0], "* SEARCH 1 2");

    let fetch = c
        .imap("a4 FETCH 1 (FLAGS BODY[HEADER.FIELDS (Subject)] BODY[TEXT])")
        .await
        .join("\n");
    assert!(
        fetch.contains("Subject: Invite") && !fetch.contains("To: bob"),
        "{fetch}"
    );
    assert!(
        fetch.contains("Join: http://x/accept?token=1") && fetch.contains("FLAGS (\\Seen)"),
        "{fetch}"
    );
    assert_eq!(c.imap("a5 SEARCH UNSEEN").await[0], "* SEARCH 2");
    assert!(
        c.imap("a6 UID FETCH 3 (UID RFC822.SIZE)").await[0]
            .starts_with("* 2 FETCH (UID 3 RFC822.SIZE ")
    );
    assert_eq!(
        c.imap("a7 UID SEARCH SUBJECT \"second\"").await[0],
        "* SEARCH 3"
    );
    let envelope = c.imap("a8 FETCH 2 ENVELOPE").await.join("\n");
    assert!(
        envelope.contains("\"Second\" ((\"Portal\" NIL \"no-reply\" \"waf.local\"))"),
        "{envelope}"
    );
    // Cờ lạ không được chen vào câu trả lời.
    assert!(c.imap("a9 STORE 1 +FLAGS (a{9})").await[0].starts_with("a9 BAD"));
    let stored = c.imap("b1 STORE 1 +FLAGS (\\Deleted)").await.join("\n");
    assert!(stored.contains("\\Deleted"), "{stored}");
    assert_eq!(c.imap("b2 EXPUNGE").await[0], "* 1 EXPUNGE");
    assert!(c.imap("b3 STATUS INBOX (MESSAGES)").await[0].contains("MESSAGES 1 "));
    c.imap("b4 LOGOUT").await;
    assert_eq!(storage.list_mails(None, 10).await.unwrap().len(), 3);
}

#[tokio::test]
async fn imap_literals_carry_the_exact_bytes() {
    let storage = seeded().await;
    let raw = storage.get_mail_raw(1).await.unwrap().unwrap();
    let mut c = start(storage, "imap", None).await;
    c.imap("a1 LOGIN bob@acme.io pw").await;
    c.imap("a2 EXAMINE INBOX").await;
    let lines = c.imap("a3 FETCH 1 BODY[]").await;
    assert_eq!(lines[0], format!("* 1 FETCH (BODY[] {{{}}}", raw.len()));
    let body = lines[1..lines.len() - 2].join("\r\n") + "\r\n";
    assert_eq!(body.as_bytes(), raw);
    // EXAMINE là chỉ đọc: không đánh dấu đã đọc.
    assert_eq!(c.imap("a4 SEARCH UNSEEN").await[0], "* SEARCH 1 2");
}

#[tokio::test]
async fn the_wildcard_reads_all_and_new_mail_is_announced_on_noop_and_idle() {
    let storage = seeded().await;
    let mut c = start(storage.clone(), "imap", None).await;
    c.imap("a1 LOGIN * pw").await;
    assert!(
        c.imap("a2 SELECT INBOX")
            .await
            .contains(&"* 3 EXISTS".to_owned())
    );
    storage
        .save_mail(mail("new@acme.io", "Late", "body"), 50)
        .await
        .unwrap();
    assert!(c.imap("a3 NOOP").await.contains(&"* 4 EXISTS".to_owned()));

    assert!(c.ask("a4 IDLE").await.starts_with('+'));
    storage
        .save_mail(mail("new@acme.io", "Later", "body"), 50)
        .await
        .unwrap();
    assert_eq!(c.line().await, "* 5 EXISTS");
    assert_eq!(c.line().await, "* 0 RECENT");
    assert!(c.ask("DONE").await.starts_with("a4 OK"));
}

#[tokio::test]
async fn imap_refuses_before_login_and_unknown_mailboxes() {
    let mut c = start(seeded().await, "imap", None).await;
    assert!(c.imap("t1 SELECT INBOX").await[0].starts_with("t1 NO"));
    c.imap("t2 LOGIN bob@acme.io pw").await;
    assert!(c.imap("t3 SELECT Drafts").await[0].starts_with("t3 NO"));
    assert!(c.imap("t4 FETCH 1 FLAGS").await[0].starts_with("t4 NO"));
}

#[tokio::test]
async fn with_a_token_only_that_password_opens_a_mailbox() {
    let storage = seeded().await;
    let mut c = start(storage.clone(), "imap", Some("s3cret")).await;
    assert!(c.imap("a1 LOGIN * wrong").await[0].starts_with("a1 NO"));
    assert!(c.imap("a2 SELECT INBOX").await[0].starts_with("a2 NO"));
    // AUTHENTICATE PLAIN: "\0bob@acme.io\0s3cret"
    assert!(c.ask("a3 AUTHENTICATE PLAIN").await.starts_with('+'));
    assert!(
        c.ask("AGJvYkBhY21lLmlvAHMzY3JldA==")
            .await
            .starts_with("a3 OK")
    );

    let mut c = start(storage, "pop3", Some("s3cret")).await;
    c.ask("USER *").await;
    assert!(c.ask("PASS wrong").await.starts_with("-ERR"));
    assert!(c.ask("STAT").await.starts_with("-ERR"));
    c.ask("USER *").await;
    assert_eq!(c.ask("PASS s3cret").await, "+OK 3 messages");
}
