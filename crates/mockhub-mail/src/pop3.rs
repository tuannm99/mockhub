//! POP3 (RFC 1939) trên các hộp thư của [`crate::mailbox`]: `USER, PASS, STAT, LIST, UIDL, RETR,
//! TOP, DELE, RSET, NOOP, CAPA, QUIT`. Không TLS. `DELE` có hiệu lực khi `QUIT`.
use std::{collections::BTreeSet, sync::Arc, time::Duration};

use mockhub_core::{domain::mail::MailboxEntry, errors::Result};
use mockhub_storage::{Storage, StorageError};
use tokio::{
    io::{AsyncWrite, BufReader},
    net::{TcpListener, TcpStream},
};

use crate::{
    mailbox::{Access, MAX_LINE_BYTES, accept_loop, split_message, write_all},
    smtp::{Line, read_line},
};

const IDLE: Duration = Duration::from_secs(300);

pub async fn serve(listener: TcpListener, storage: Arc<dyn Storage>, access: Access) -> Result<()> {
    accept_loop(listener, "pop3", move |stream| {
        session(stream, storage.clone(), access.clone())
    })
    .await
}

/// Khối kết thúc bằng dòng `.`, chèn thêm dấu chấm cho dòng bắt đầu bằng `.`.
fn multiline(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 8);
    let text = text.strip_suffix(b"\r\n").unwrap_or(text);
    if !text.is_empty() {
        for line in text.split(|b| *b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.starts_with(b".") {
                out.push(b'.');
            }
            out.extend_from_slice(line);
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(b".\r\n");
    out
}

/// Header cộng `lines` dòng đầu của thân.
fn top(raw: &[u8], lines: usize) -> Vec<u8> {
    let (mut out, body) = split_message(raw);
    for line in body.split_inclusive(|b| *b == b'\n').take(lines) {
        out.extend_from_slice(line);
    }
    out
}

#[derive(Default)]
struct State {
    user: String,
    authed: bool,
    /// Ảnh chụp hộp thư lúc đăng nhập; số thư = chỉ số + 1.
    mails: Vec<MailboxEntry>,
    deleted: BTreeSet<usize>,
}

impl State {
    /// Chỉ số của thư số `arg`, nếu có và chưa bị `DELE`.
    fn pick(&self, arg: &str) -> Option<usize> {
        let index = arg.trim().parse::<usize>().ok()?.checked_sub(1)?;
        (index < self.mails.len() && !self.deleted.contains(&index)).then_some(index)
    }

    fn live(&self) -> impl Iterator<Item = (usize, &MailboxEntry)> {
        self.mails
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.deleted.contains(i))
    }

    /// `LIST` / `UIDL`: một thư, hoặc cả danh sách.
    fn listing(&self, arg: &str, each: impl Fn(usize, &MailboxEntry) -> String) -> Vec<u8> {
        if !arg.trim().is_empty() {
            return match self.pick(arg) {
                Some(i) => format!("+OK {}\r\n", each(i + 1, &self.mails[i])).into_bytes(),
                None => NO_SUCH.into(),
            };
        }
        let rows: String = self.live().map(|(i, m)| each(i + 1, m) + "\r\n").collect();
        let mut out = b"+OK\r\n".to_vec();
        out.extend(multiline(rows.as_bytes()));
        out
    }
}

const NO_SUCH: &[u8] = b"-ERR no such message\r\n";

async fn line(writer: &mut (impl AsyncWrite + Unpin), text: &str) -> std::io::Result<()> {
    write_all(writer, format!("{text}\r\n").as_bytes()).await
}

async fn session(
    stream: TcpStream,
    storage: Arc<dyn Storage>,
    access: Access,
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut state = State::default();
    line(&mut write, "+OK mockhub POP3 ready").await?;
    loop {
        let Ok(read) = tokio::time::timeout(IDLE, read_line(&mut reader, MAX_LINE_BYTES)).await
        else {
            return Ok(());
        };
        let text = match read? {
            Line::Closed => return Ok(()),
            Line::TooLong => {
                line(&mut write, "-ERR line too long").await?;
                continue;
            }
            Line::Full(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        };
        let (verb, arg) = text.split_once(' ').unwrap_or((&text, ""));
        let verb = verb.trim().to_ascii_uppercase();
        let reply = match command(&mut state, storage.as_ref(), &access, &verb, arg).await {
            Ok(reply) => reply,
            Err(e) => {
                tracing::error!("pop3 storage: {e}");
                b"-ERR storage error\r\n".to_vec()
            }
        };
        write_all(&mut write, &reply).await?;
        if verb == "QUIT" {
            return Ok(());
        }
    }
}

async fn command(
    state: &mut State,
    storage: &dyn Storage,
    access: &Access,
    verb: &str,
    arg: &str,
) -> std::result::Result<Vec<u8>, StorageError> {
    let ok = |text: String| format!("+OK {text}\r\n").into_bytes();
    let err = |text: &str| format!("-ERR {text}\r\n").into_bytes();
    Ok(match verb {
        "CAPA" => {
            let mut out = ok("capability list follows".into());
            out.extend(multiline(
                b"USER\r\nUIDL\r\nTOP\r\nPIPELINING\r\nIMPLEMENTATION mockhub",
            ));
            out
        }
        "QUIT" => {
            if state.authed {
                let ids: Vec<i64> = state.deleted.iter().map(|i| state.mails[*i].id).collect();
                storage.hide_from_mailbox(&state.user, &ids).await?;
            }
            ok("bye".into())
        }
        "USER" | "PASS" if state.authed => err("already authenticated"),
        "USER" => {
            state.user = arg.trim().to_owned();
            ok("send PASS".into())
        }
        "PASS" if state.user.is_empty() => err("send USER first"),
        // Mật khẩu lấy nguyên văn: có thể chứa dấu cách.
        "PASS" if !access.allows(&state.user, arg) => {
            state.user.clear();
            err("[AUTH] wrong password")
        }
        "PASS" => {
            state.mails = storage.list_mailbox(&state.user).await?;
            state.authed = true;
            ok(format!("{} messages", state.mails.len()))
        }
        _ if !state.authed => err("not authenticated"),
        "STAT" => {
            let (count, size) = state
                .live()
                .fold((0, 0), |(n, total), (_, m)| (n + 1, total + m.size));
            ok(format!("{count} {size}"))
        }
        "LIST" => state.listing(arg, |n, m| format!("{n} {}", m.size)),
        "UIDL" => state.listing(arg, |n, m| format!("{n} mh{}", m.id)),
        "RETR" | "TOP" => {
            let (number, rest) = arg.trim().split_once(' ').unwrap_or((arg.trim(), ""));
            let Some(index) = state.pick(number) else {
                return Ok(NO_SUCH.into());
            };
            let lines = rest.trim().parse::<usize>();
            if verb == "TOP" && lines.is_err() {
                return Ok(err("syntax: TOP msg lines"));
            }
            let id = state.mails[index].id;
            // Thư có thể đã bị bỏ vì quá `mail.capacity` hoặc bị xoá qua REST.
            let Some(raw) = storage.get_mail_raw(id).await? else {
                return Ok(NO_SUCH.into());
            };
            let (mut out, text) = if verb == "RETR" {
                storage
                    .set_mailbox_flag(&state.user, id, "\\Seen", true)
                    .await?;
                (ok(format!("{} octets", raw.len())), raw)
            } else {
                (ok(String::new()), top(&raw, lines.unwrap_or(0)))
            };
            out.extend(multiline(&text));
            out
        }
        "DELE" => match state.pick(arg) {
            Some(index) => {
                state.deleted.insert(index);
                ok(format!("message {} deleted", index + 1))
            }
            None => NO_SUCH.into(),
        },
        "RSET" => {
            state.deleted.clear();
            ok(String::new())
        }
        "NOOP" => ok(String::new()),
        _ => err("unknown command"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_are_stuffed_and_top_counts_body_lines() {
        assert_eq!(multiline(b"a\r\n.b\r\n"), b"a\r\n..b\r\n.\r\n");
        assert_eq!(multiline(b""), b".\r\n");
        let raw = b"S: x\r\n\r\none\r\ntwo\r\n";
        assert_eq!(top(raw, 0), b"S: x\r\n\r\n");
        assert_eq!(top(raw, 1), b"S: x\r\n\r\none\r\n");
        assert_eq!(top(raw, 9), raw);
    }
}
