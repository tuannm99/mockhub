//! SMTP nhận thư (RFC 5321, phần đủ cho một mock). Nhận mọi địa chỉ, không chuyển tiếp đi đâu.
//! Không STARTTLS; `AUTH PLAIN/LOGIN` nhận mọi tài khoản.
//!
//! [`Session`] là máy trạng thái thuần (không I/O); [`serve`] đọc/ghi socket.
use std::{sync::Arc, time::Duration};

use mockhub_core::errors::Result;
use mockhub_storage::Storage;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

use crate::mime;

pub const MAX_MESSAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_COMMAND_BYTES: usize = 8 * 1024;
const MAX_RECIPIENTS: usize = 200;
const IDLE: Duration = Duration::from_secs(300);
const HOSTNAME: &str = "mockhub";

#[derive(Debug, PartialEq)]
pub enum Next {
    /// Gửi dòng trả lời, đọc lệnh tiếp.
    Reply(String),
    /// Gửi dòng trả lời rồi đọc phần DATA.
    Data(String),
    /// Gửi dòng trả lời, rồi đọc `n` dòng của AUTH (nội dung bị bỏ qua), rồi báo thành công.
    Auth(Vec<&'static str>),
    Quit(String),
}

#[derive(Debug, Default)]
pub struct Session {
    from: Option<String>,
    recipients: Vec<String>,
}

fn reply(text: &str) -> Next {
    Next::Reply(text.to_owned())
}

/// Địa chỉ trong `FROM:<a@b> SIZE=1` / `TO:<a@b>`.
fn path_arg<'a>(arg: &'a str, prefix: &str) -> Option<&'a str> {
    let head = arg.get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let rest = arg[prefix.len()..].trim_start();
    match rest.strip_prefix('<') {
        Some(inner) => inner.find('>').map(|end| &inner[..end]),
        None => rest.split(' ').next(),
    }
}

impl Session {
    pub fn command(&mut self, line: &str) -> Next {
        let (verb, arg) = line.split_once(' ').unwrap_or((line, ""));
        match verb.to_ascii_uppercase().as_str() {
            "EHLO" => {
                self.reset();
                Next::Reply(format!(
                    "250-{HOSTNAME}\r\n250-SIZE {MAX_MESSAGE_BYTES}\r\n250-8BITMIME\r\n250-AUTH PLAIN LOGIN\r\n250 SMTPUTF8"
                ))
            }
            "HELO" => {
                self.reset();
                Next::Reply(format!("250 {HOSTNAME}"))
            }
            "AUTH" => {
                let (mechanism, initial) = arg.split_once(' ').unwrap_or((arg, ""));
                match mechanism.to_ascii_uppercase().as_str() {
                    "PLAIN" if !initial.is_empty() => reply("235 authenticated"),
                    "PLAIN" => Next::Auth(vec!["334 "]),
                    // "Username:", "Password:"
                    "LOGIN" if !initial.is_empty() => Next::Auth(vec!["334 UGFzc3dvcmQ6"]),
                    "LOGIN" => Next::Auth(vec!["334 VXNlcm5hbWU6", "334 UGFzc3dvcmQ6"]),
                    _ => reply("504 unsupported mechanism"),
                }
            }
            "MAIL" => match path_arg(arg, "FROM:") {
                // `<>` (thư trả lại) là hợp lệ.
                Some(address) => {
                    self.reset();
                    self.from = Some(address.to_owned());
                    reply("250 OK")
                }
                None => reply("501 syntax: MAIL FROM:<address>"),
            },
            "RCPT" => match path_arg(arg, "TO:") {
                _ if self.from.is_none() => reply("503 need MAIL first"),
                _ if self.recipients.len() >= MAX_RECIPIENTS => reply("452 too many recipients"),
                Some(address) if !address.is_empty() => {
                    self.recipients.push(address.to_owned());
                    reply("250 OK")
                }
                _ => reply("501 syntax: RCPT TO:<address>"),
            },
            "DATA" if self.recipients.is_empty() => reply("503 need RCPT first"),
            "DATA" => Next::Data("354 end data with <CR><LF>.<CR><LF>".to_owned()),
            "RSET" => {
                self.reset();
                reply("250 OK")
            }
            "NOOP" => reply("250 OK"),
            "QUIT" => Next::Quit("221 bye".to_owned()),
            _ => reply("502 command not implemented"),
        }
    }

    fn reset(&mut self) {
        self.from = None;
        self.recipients.clear();
    }

    /// Lấy phong bì đã gom và sẵn sàng cho thư kế tiếp.
    fn take_envelope(&mut self) -> (String, Vec<String>) {
        (
            self.from.take().unwrap_or_default(),
            std::mem::take(&mut self.recipients),
        )
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Line {
    Full(Vec<u8>),
    /// Dài quá giới hạn; đã đọc bỏ tới hết dòng.
    TooLong,
    Closed,
}

/// Đọc một dòng (bỏ CRLF) mà không bao giờ giữ quá `max` byte trong bộ nhớ.
pub(crate) async fn read_line(
    reader: &mut (impl AsyncBufRead + Unpin),
    max: usize,
) -> std::io::Result<Line> {
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(Line::Closed);
        }
        let end = chunk.iter().position(|b| *b == b'\n');
        let used = end.map_or(chunk.len(), |i| i + 1);
        if !too_long {
            line.extend_from_slice(&chunk[..used]);
            if line.len() > max + 2 {
                too_long = true;
                line.clear();
            }
        }
        reader.consume(used);
        if end.is_some() {
            if too_long {
                return Ok(Line::TooLong);
            }
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Line::Full(line));
        }
    }
}

/// Đọc phần DATA tới dòng `.`, bỏ dot-stuffing. `Ok(Err(()))`: quá cỡ (vẫn đọc hết để phiên không lệch).
async fn read_data(
    reader: &mut (impl AsyncBufRead + Unpin),
    max: usize,
) -> std::io::Result<Option<std::result::Result<Vec<u8>, ()>>> {
    let mut data = Vec::new();
    let mut too_big = false;
    loop {
        let Ok(line) = tokio::time::timeout(IDLE, read_line(reader, max)).await else {
            return Ok(None);
        };
        match line? {
            Line::Closed => return Ok(None),
            Line::TooLong => too_big = true,
            Line::Full(line) if line == b"." => {
                return Ok(Some(if too_big { Err(()) } else { Ok(data) }));
            }
            Line::Full(line) => {
                let body = line.strip_prefix(b".").unwrap_or(&line);
                if too_big || data.len() + body.len() + 2 > max {
                    too_big = true;
                    data = Vec::new();
                } else {
                    data.extend_from_slice(body);
                    data.extend_from_slice(b"\r\n");
                }
            }
        }
    }
}

async fn send(writer: &mut (impl AsyncWrite + Unpin), text: &str) -> std::io::Result<()> {
    writer.write_all(format!("{text}\r\n").as_bytes()).await
}

async fn session(stream: TcpStream, storage: Arc<dyn Storage>, keep: u32) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut state = Session::default();
    send(&mut write, &format!("220 {HOSTNAME} ESMTP")).await?;
    loop {
        let Ok(line) = tokio::time::timeout(IDLE, read_line(&mut reader, MAX_COMMAND_BYTES)).await
        else {
            return send(&mut write, "421 idle timeout").await;
        };
        let line = match line? {
            Line::Closed => return Ok(()),
            Line::TooLong => {
                send(&mut write, "500 line too long").await?;
                continue;
            }
            Line::Full(line) => String::from_utf8_lossy(&line).into_owned(),
        };
        match state.command(&line) {
            Next::Reply(text) => send(&mut write, &text).await?,
            Next::Quit(text) => return send(&mut write, &text).await,
            Next::Auth(prompts) => {
                for prompt in prompts {
                    send(&mut write, prompt).await?;
                    match tokio::time::timeout(IDLE, read_line(&mut reader, MAX_COMMAND_BYTES))
                        .await
                    {
                        Ok(Ok(Line::Full(_))) => {}
                        _ => return Ok(()),
                    }
                }
                send(&mut write, "235 authenticated").await?;
            }
            Next::Data(text) => {
                send(&mut write, &text).await?;
                let Some(data) = read_data(&mut reader, MAX_MESSAGE_BYTES).await? else {
                    return Ok(());
                };
                let (from, recipients) = state.take_envelope();
                let Ok(raw) = data else {
                    send(&mut write, "552 message too large").await?;
                    continue;
                };
                // Chỉ báo 250 sau khi đã lưu được.
                match storage
                    .save_mail(mime::to_new_mail(from, &recipients, raw), keep)
                    .await
                {
                    Ok(mail) => {
                        tracing::info!(id = mail.id, from = %mail.from, ?recipients, subject = %mail.subject, "mail received");
                        send(&mut write, &format!("250 OK queued as {}", mail.id)).await?;
                    }
                    Err(e) => {
                        tracing::error!("saving mail: {e}");
                        send(&mut write, "451 could not store the message").await?;
                    }
                }
            }
        }
    }
}

pub async fn serve(listener: TcpListener, storage: Arc<dyn Storage>, keep: u32) -> Result<()> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(got) => got,
            Err(e) => {
                tracing::warn!("smtp accept: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let storage = storage.clone();
        tokio::spawn(async move {
            if let Err(e) = session(stream, storage, keep).await {
                tracing::debug!("smtp {peer}: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_out_of_order_are_refused() {
        let mut s = Session::default();
        assert_eq!(
            s.command("RCPT TO:<b@mock.test>"),
            reply("503 need MAIL first")
        );
        assert_eq!(s.command("DATA"), reply("503 need RCPT first"));
        assert_eq!(
            s.command("mail from:<a@mock.test> SIZE=10"),
            reply("250 OK")
        );
        assert_eq!(s.command("rcpt to:<b@mock.test>"), reply("250 OK"));
        assert!(matches!(s.command("DATA"), Next::Data(_)));
        assert_eq!(
            s.take_envelope(),
            ("a@mock.test".to_owned(), vec!["b@mock.test".to_owned()])
        );
        assert_eq!(s.command("DATA"), reply("503 need RCPT first"));
    }

    #[test]
    fn rset_and_a_new_mail_drop_the_old_envelope() {
        let mut s = Session::default();
        s.command("MAIL FROM:<>");
        s.command("RCPT TO:<b@mock.test>");
        s.command("RSET");
        assert_eq!(s.command("DATA"), reply("503 need RCPT first"));
    }

    #[test]
    fn bad_paths_and_unknown_commands() {
        let mut s = Session::default();
        assert!(matches!(s.command("MAIL a@mock.test"), Next::Reply(r) if r.starts_with("501")));
        assert!(
            matches!(s.command("MAIL FROM:<a@mock.test"), Next::Reply(r) if r.starts_with("501"))
        );
        assert!(matches!(s.command("STARTTLS"), Next::Reply(r) if r.starts_with("502")));
        assert!(matches!(s.command("é"), Next::Reply(r) if r.starts_with("502")));
        assert!(
            matches!(s.command("MAIL ééééé"), Next::Reply(r) if r.starts_with("501")),
            "no panic on a split char"
        );
    }

    #[tokio::test]
    async fn data_is_unstuffed_and_a_big_one_is_drained() {
        let mut input: &[u8] = b"a\r\n..dot\r\n.\r\nNOOP\r\n";
        let data = read_data(&mut input, 100).await.unwrap().unwrap().unwrap();
        assert_eq!(data, b"a\r\n.dot\r\n");
        assert_eq!(
            read_line(&mut input, 100).await.unwrap(),
            Line::Full(b"NOOP".to_vec())
        );

        let mut input: &[u8] = b"0123456789\r\nabc\r\n.\r\nNOOP\r\n";
        assert_eq!(read_data(&mut input, 8).await.unwrap(), Some(Err(())));
        assert_eq!(
            read_line(&mut input, 100).await.unwrap(),
            Line::Full(b"NOOP".to_vec())
        );
    }
}
