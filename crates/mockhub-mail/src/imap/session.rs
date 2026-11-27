//! Một kết nối IMAP.
use std::{sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use mockhub_core::domain::mail::{Mail, MailboxEntry};
use mockhub_storage::{Storage, StorageError};
use tokio::{
    io::BufReader,
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
};

use super::{
    fetch,
    parse::{fetch_items, resolve_set, tokens, unquote},
};
use crate::{
    mailbox::{Access, MAX_LINE_BYTES, write_all},
    smtp::{Line, read_line},
};

const IDLE_LIMIT: Duration = Duration::from_secs(30 * 60);
/// Trong `IDLE`, bao lâu xem hộp thư một lần.
const IDLE_POLL: Duration = Duration::from_secs(1);
const FLAGS: &str = r"\Seen \Answered \Flagged \Deleted \Draft";
const CAPABILITIES: &str = "IMAP4rev1 AUTH=PLAIN IDLE UIDPLUS UNSELECT NAMESPACE";

#[derive(Debug, thiserror::Error)]
enum Fail {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

type Done = Result<(), Fail>;
type Reader = BufReader<OwnedReadHalf>;

/// `INBOX` đang chọn, cũ nhất trước; số thứ tự = chỉ số + 1.
struct Selected {
    mails: Vec<MailboxEntry>,
    read_only: bool,
}

struct Session {
    storage: Arc<dyn Storage>,
    access: Access,
    uid_validity: u32,
    writer: OwnedWriteHalf,
    user: Option<String>,
    selected: Option<Selected>,
}

fn has_flag(entry: &MailboxEntry, flag: &str) -> bool {
    entry.flags.iter().any(|f| f.eq_ignore_ascii_case(flag))
}

/// Cờ client gửi lên đi thẳng vào câu trả lời, nên chỉ nhận atom hợp lệ.
fn valid_flag(flag: &str) -> bool {
    let name = flag.strip_prefix('\\').unwrap_or(flag);
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_graphic() && !"(){%*\"\\]".contains(c))
}

/// `None`: kết nối đóng hoặc im lặng quá lâu.
async fn next_line(reader: &mut Reader) -> std::io::Result<Option<String>> {
    let Ok(line) = tokio::time::timeout(IDLE_LIMIT, read_line(reader, MAX_LINE_BYTES)).await else {
        return Ok(None);
    };
    Ok(match line? {
        Line::Full(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Line::TooLong => Some(String::new()),
        Line::Closed => None,
    })
}

pub(super) async fn run(
    stream: TcpStream,
    storage: Arc<dyn Storage>,
    access: Access,
    uid_validity: u32,
) -> std::io::Result<()> {
    let (read, writer) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut session = Session {
        storage,
        access,
        uid_validity,
        writer,
        user: None,
        selected: None,
    };
    let greeting = format!("* OK [CAPABILITY {CAPABILITIES}] mockhub IMAP ready");
    write_all(&mut session.writer, format!("{greeting}\r\n").as_bytes()).await?;
    while let Some(line) = next_line(&mut reader).await? {
        let (tag, rest) = line.split_once(' ').unwrap_or((&line, ""));
        let (command, args) = rest.split_once(' ').unwrap_or((rest, ""));
        let result = if tag.is_empty() || command.is_empty() {
            session.out("* BAD empty command").await.map(|()| true)
        } else {
            session
                .dispatch(&mut reader, tag, &command.to_ascii_uppercase(), args.trim())
                .await
        };
        match result {
            Ok(true) => {}
            Ok(false) => return Ok(()),
            Err(Fail::Io(e)) => return Err(e),
            Err(Fail::Storage(e)) => {
                tracing::error!("imap storage: {e}");
                let reply = format!("{tag} NO storage error\r\n");
                write_all(&mut session.writer, reply.as_bytes()).await?;
            }
        }
    }
    Ok(())
}

impl Session {
    async fn out(&mut self, line: &str) -> Done {
        write_all(&mut self.writer, format!("{line}\r\n").as_bytes()).await?;
        Ok(())
    }

    async fn tagged(&mut self, tag: &str, status: &str, text: &str) -> Done {
        self.out(&format!("{tag} {status} {text}")).await
    }

    fn user(&self) -> String {
        self.user.clone().unwrap_or_default()
    }

    async fn login(&mut self, tag: &str, command: &str, user: String, password: &str) -> Done {
        if self.access.allows(&user, password) {
            self.user = Some(user);
            self.selected = None;
            self.tagged(tag, "OK", &format!("{command} completed"))
                .await
        } else {
            self.tagged(tag, "NO", "[AUTHENTICATIONFAILED] wrong user or password")
                .await
        }
    }

    /// `Ok(false)`: đóng kết nối.
    async fn dispatch(
        &mut self,
        reader: &mut Reader,
        tag: &str,
        command: &str,
        args: &str,
    ) -> Result<bool, Fail> {
        match command {
            "CAPABILITY" => {
                self.out(&format!("* CAPABILITY {CAPABILITIES}")).await?;
                self.tagged(tag, "OK", "CAPABILITY completed").await?;
            }
            "NOOP" => {
                self.refresh().await?;
                self.tagged(tag, "OK", "NOOP completed").await?;
            }
            "LOGOUT" => {
                self.out("* BYE mockhub closing").await?;
                self.tagged(tag, "OK", "LOGOUT completed").await?;
                return Ok(false);
            }
            "LOGIN" => match tokens(args).as_slice() {
                [user, password, ..] => {
                    self.login(tag, "LOGIN", unquote(user), &unquote(password))
                        .await?;
                }
                _ => {
                    self.tagged(tag, "BAD", "LOGIN needs a user and a password")
                        .await?;
                }
            },
            "AUTHENTICATE" => {
                let (mechanism, initial) = args.split_once(' ').unwrap_or((args, ""));
                if !mechanism.eq_ignore_ascii_case("PLAIN") {
                    self.tagged(tag, "NO", "only PLAIN is supported").await?;
                    return Ok(true);
                }
                let mut response = initial.trim().to_owned();
                if response.is_empty() {
                    self.out("+ ").await?;
                    let Some(line) = next_line(reader).await? else {
                        return Ok(false);
                    };
                    response = line.trim().to_owned();
                }
                // authzid NUL authcid NUL password
                let decoded = STANDARD
                    .decode(response)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_default();
                match decoded.split('\0').collect::<Vec<_>>().as_slice() {
                    [identity, user, password] => {
                        let user = if user.is_empty() { identity } else { user };
                        self.login(tag, "AUTHENTICATE", (*user).to_owned(), password)
                            .await?;
                    }
                    _ => {
                        self.tagged(tag, "BAD", "malformed PLAIN response").await?;
                    }
                }
            }
            _ if self.user.is_none() => self.tagged(tag, "NO", "log in first").await?,
            "IDLE" => return self.idle(reader, tag).await,
            _ => self.authenticated(tag, command, args).await?,
        }
        Ok(true)
    }

    async fn authenticated(&mut self, tag: &str, command: &str, args: &str) -> Done {
        let completed = format!("{command} completed");
        match command {
            "NAMESPACE" => {
                self.out(r#"* NAMESPACE (("" "/")) NIL NIL"#).await?;
                self.tagged(tag, "OK", &completed).await
            }
            "LIST" | "LSUB" => {
                // Pattern rỗng hỏi dấu phân cấp.
                let root = tokens(args).get(1).is_some_and(|p| unquote(p).is_empty());
                let entry = if root {
                    r#"(\Noselect) "/" """#
                } else {
                    r#"(\HasNoChildren) "/" "INBOX""#
                };
                self.out(&format!("* {command} {entry}")).await?;
                self.tagged(tag, "OK", &completed).await
            }
            "SUBSCRIBE" | "UNSUBSCRIBE" => self.tagged(tag, "OK", &completed).await,
            "CREATE" | "DELETE" | "RENAME" | "APPEND" | "COPY" => {
                let text = "mockhub has only INBOX and is read-only apart from flags";
                self.tagged(tag, "NO", text).await
            }
            "STATUS" => {
                let mails = self.storage.list_mailbox(&self.user()).await?;
                let unseen = mails.iter().filter(|m| !has_flag(m, "\\Seen")).count();
                let next = self.storage.next_mail_id().await?;
                self.out(&format!(
                    "* STATUS \"INBOX\" (MESSAGES {} UIDNEXT {next} UIDVALIDITY {} UNSEEN {unseen})",
                    mails.len(),
                    self.uid_validity
                ))
                .await?;
                self.tagged(tag, "OK", &completed).await
            }
            "SELECT" | "EXAMINE" => self.select(tag, command, args).await,
            "CHECK" => {
                self.refresh().await?;
                self.tagged(tag, "OK", &completed).await
            }
            "UNSELECT" => {
                self.selected = None;
                self.tagged(tag, "OK", &completed).await
            }
            "CLOSE" => {
                if self.selected.as_ref().is_some_and(|s| !s.read_only) {
                    self.expunge(false).await?;
                }
                self.selected = None;
                self.tagged(tag, "OK", &completed).await
            }
            "EXPUNGE" => match &self.selected {
                None => self.tagged(tag, "NO", "select a mailbox first").await,
                Some(s) if s.read_only => self.tagged(tag, "NO", "mailbox is read-only").await,
                Some(_) => {
                    self.expunge(true).await?;
                    self.tagged(tag, "OK", &completed).await
                }
            },
            "FETCH" | "SEARCH" | "STORE" => self.messages(tag, command, args, false).await,
            "UID" => {
                let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
                let sub = sub.to_ascii_uppercase();
                if matches!(sub.as_str(), "FETCH" | "SEARCH" | "STORE") {
                    self.messages(tag, &sub, rest.trim(), true).await
                } else {
                    self.tagged(tag, "BAD", "unsupported UID command").await
                }
            }
            _ => self.tagged(tag, "BAD", "unknown command").await,
        }
    }

    async fn select(&mut self, tag: &str, command: &str, args: &str) -> Done {
        self.selected = None;
        let name = tokens(args).first().map(|t| unquote(t));
        if !name.is_some_and(|n| n.eq_ignore_ascii_case("INBOX")) {
            return self.tagged(tag, "NO", "no such mailbox (only INBOX)").await;
        }
        let mails = self.storage.list_mailbox(&self.user()).await?;
        let next = self.storage.next_mail_id().await?;
        let lines = [
            format!("* FLAGS ({FLAGS})"),
            format!("* OK [PERMANENTFLAGS ({FLAGS} \\*)] flags are kept"),
            format!("* {} EXISTS", mails.len()),
            "* 0 RECENT".to_owned(),
            format!("* OK [UIDVALIDITY {}] valid", self.uid_validity),
            format!("* OK [UIDNEXT {next}] next"),
        ];
        let read_only = command == "EXAMINE";
        self.selected = Some(Selected { mails, read_only });
        for line in lines {
            self.out(&line).await?;
        }
        let mode = if read_only { "READ-ONLY" } else { "READ-WRITE" };
        self.tagged(tag, "OK", &format!("[{mode}] {command} completed"))
            .await
    }

    /// Báo thư mới đến từ lần xem trước.
    async fn refresh(&mut self) -> Done {
        let user = self.user();
        let Some(selected) = &mut self.selected else {
            return Ok(());
        };
        let newest = selected.mails.last().map_or(0, |m| m.id);
        let fresh: Vec<_> = self
            .storage
            .list_mailbox(&user)
            .await?
            .into_iter()
            .filter(|m| m.id > newest)
            .collect();
        if fresh.is_empty() {
            return Ok(());
        }
        selected.mails.extend(fresh);
        let count = selected.mails.len();
        self.out(&format!("* {count} EXISTS")).await?;
        self.out("* 0 RECENT").await
    }

    async fn idle(&mut self, reader: &mut Reader, tag: &str) -> Result<bool, Fail> {
        if self.selected.is_none() {
            self.tagged(tag, "NO", "select a mailbox first").await?;
            return Ok(true);
        }
        self.out("+ idling").await?;
        // `read_line` không an toàn khi bị huỷ giữa chừng, nên giữ một future duy nhất.
        let done = next_line(reader);
        tokio::pin!(done);
        let mut tick = tokio::time::interval(IDLE_POLL);
        loop {
            tokio::select! {
                line = &mut done => {
                    return match line? {
                        Some(line) if line.trim().eq_ignore_ascii_case("DONE") => {
                            self.tagged(tag, "OK", "IDLE terminated").await?;
                            Ok(true)
                        }
                        Some(_) => {
                            self.tagged(tag, "BAD", "expected DONE").await?;
                            Ok(true)
                        }
                        None => Ok(false),
                    };
                }
                _ = tick.tick() => self.refresh().await?,
            }
        }
    }

    /// Ẩn các thư `\Deleted` khỏi hộp này. `announce`: gửi các dòng `* n EXPUNGE`.
    async fn expunge(&mut self, announce: bool) -> Done {
        let user = self.user();
        let Some(selected) = &mut self.selected else {
            return Ok(());
        };
        let (mut kept, mut gone, mut numbers) = (Vec::new(), Vec::new(), Vec::new());
        for entry in selected.mails.drain(..) {
            if has_flag(&entry, "\\Deleted") {
                gone.push(entry.id);
                // Mỗi lần xoá đánh số lại phần sau.
                numbers.push(kept.len() + 1);
            } else {
                kept.push(entry);
            }
        }
        selected.mails = kept;
        self.storage.hide_from_mailbox(&user, &gone).await?;
        if announce {
            for number in numbers {
                self.out(&format!("* {number} EXPUNGE")).await?;
            }
        }
        Ok(())
    }

    /// Khoá của từng thư: UID hoặc số thứ tự.
    fn keys(&self, uid: bool) -> Vec<i64> {
        let mails = self.selected.as_ref().map_or(&[][..], |s| &s.mails);
        mails
            .iter()
            .enumerate()
            .map(|(i, m)| if uid { m.id } else { i as i64 + 1 })
            .collect()
    }

    async fn messages(&mut self, tag: &str, command: &str, args: &str, uid: bool) -> Done {
        let Some(read_only) = self.selected.as_ref().map(|s| s.read_only) else {
            return self.tagged(tag, "NO", "select a mailbox first").await;
        };
        let label = if uid {
            format!("UID {command} completed")
        } else {
            format!("{command} completed")
        };
        if command == "SEARCH" {
            let found: String = self
                .search(args, uid)
                .await?
                .iter()
                .map(|n| format!(" {n}"))
                .collect();
            self.out(&format!("* SEARCH{found}")).await?;
            return self.tagged(tag, "OK", &label).await;
        }
        let (set, rest) = args.split_once(' ').unwrap_or((args, ""));
        let indexes = resolve_set(set, &self.keys(uid));
        if command == "STORE" {
            if read_only {
                return self.tagged(tag, "NO", "mailbox is read-only").await;
            }
            if !self.store(&indexes, rest.trim(), uid).await? {
                return self.tagged(tag, "BAD", "bad STORE arguments").await;
            }
        } else {
            let mut items = fetch_items(rest);
            if uid && !items.iter().any(|i| i.eq_ignore_ascii_case("UID")) {
                items.insert(0, "UID".to_owned());
            }
            for index in indexes {
                self.fetch_one(index, &items, read_only).await?;
            }
        }
        self.tagged(tag, "OK", &label).await
    }

    async fn store(&mut self, indexes: &[usize], rest: &str, uid: bool) -> Result<bool, Fail> {
        let (mode, list) = rest.split_once(' ').unwrap_or((rest, ""));
        let mode = mode.to_ascii_uppercase();
        let silent = mode.ends_with(".SILENT");
        let operation = mode.trim_end_matches(".SILENT");
        let flags: Vec<&str> = list
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split_whitespace()
            .collect();
        if !matches!(operation, "FLAGS" | "+FLAGS" | "-FLAGS")
            || !flags.iter().all(|f| valid_flag(f))
        {
            return Ok(false);
        }
        let user = self.user();
        for &index in indexes {
            let Some(entry) = self.selected.as_ref().and_then(|s| s.mails.get(index)) else {
                continue;
            };
            let id = entry.id;
            if operation == "FLAGS" {
                for old in self.storage.mailbox_flags(&user, id).await? {
                    self.storage
                        .set_mailbox_flag(&user, id, &old, false)
                        .await?;
                }
            }
            for flag in &flags {
                self.storage
                    .set_mailbox_flag(&user, id, flag, operation != "-FLAGS")
                    .await?;
            }
            let now = self.storage.mailbox_flags(&user, id).await?;
            let line = format!(
                "* {} FETCH ({}FLAGS ({}))",
                index + 1,
                if uid {
                    format!("UID {id} ")
                } else {
                    String::new()
                },
                now.join(" ")
            );
            if let Some(selected) = &mut self.selected {
                selected.mails[index].flags = now;
            }
            if !silent {
                self.out(&line).await?;
            }
        }
        Ok(true)
    }

    /// Nội dung một thư. `None`: đã bị bỏ vì quá `mail.capacity` hoặc bị xoá qua REST.
    async fn load(&self, id: i64) -> Result<Option<(Mail, Vec<u8>)>, Fail> {
        let (mail, raw) = (
            self.storage.get_mail(id).await?,
            self.storage.get_mail_raw(id).await?,
        );
        Ok(mail.zip(raw))
    }

    async fn fetch_one(&mut self, index: usize, items: &[String], read_only: bool) -> Done {
        let Some(mut entry) = self
            .selected
            .as_ref()
            .and_then(|s| s.mails.get(index))
            .cloned()
        else {
            return Ok(());
        };
        let content = if items.iter().any(|i| fetch::needs_content(i)) {
            match self.load(entry.id).await? {
                Some(content) => Some(content),
                None => return Ok(()),
            }
        } else {
            None
        };
        // Đánh dấu trước khi dựng câu trả lời, để `FLAGS` trong đó là trạng thái mới.
        let newly_seen =
            !read_only && items.iter().any(|i| fetch::marks_seen(i)) && !has_flag(&entry, "\\Seen");
        if newly_seen {
            let user = self.user();
            self.storage
                .set_mailbox_flag(&user, entry.id, "\\Seen", true)
                .await?;
            entry.flags = self.storage.mailbox_flags(&user, entry.id).await?;
            if let Some(selected) = &mut self.selected {
                selected.mails[index].flags = entry.flags.clone();
            }
        }
        let mut wanted: Vec<&str> = items.iter().map(String::as_str).collect();
        if newly_seen && !wanted.iter().any(|i| i.eq_ignore_ascii_case("FLAGS")) {
            wanted.push("FLAGS");
        }
        let mut out = format!("* {} FETCH (", index + 1).into_bytes();
        let start = out.len();
        for item in wanted {
            let mut part = Vec::new();
            match item.to_ascii_uppercase().as_str() {
                "UID" => part = format!("UID {}", entry.id).into_bytes(),
                "FLAGS" => part = format!("FLAGS ({})", entry.flags.join(" ")).into_bytes(),
                "RFC822.SIZE" => part = format!("RFC822.SIZE {}", entry.size).into_bytes(),
                _ => {
                    let Some((mail, raw)) = &content else {
                        continue;
                    };
                    if !fetch::content_item(&mut part, item, mail, raw) {
                        continue;
                    }
                }
            }
            if out.len() > start {
                out.push(b' ');
            }
            out.extend(part);
        }
        out.extend_from_slice(b")\r\n");
        write_all(&mut self.writer, &out).await?;
        Ok(())
    }

    /// `ALL, SEEN, UNSEEN, DELETED, UNDELETED, FLAGGED, UNFLAGGED, FROM, TO, SUBJECT, TEXT, BODY,
    /// UID <tập>` và tập số thứ tự, nối bằng AND. Trả số thứ tự, hoặc UID với `UID SEARCH`.
    /// Khoá không hiểu thì bỏ qua.
    async fn search(&mut self, args: &str, uid: bool) -> Result<Vec<i64>, Fail> {
        let entries = self
            .selected
            .as_ref()
            .map_or_else(Vec::new, |s| s.mails.clone());
        let mut matching = vec![true; entries.len()];
        let mut texts: Vec<(&'static str, String)> = Vec::new();
        let words = tokens(args);
        let mut words = words.iter();
        while let Some(word) = words.next() {
            let key = unquote(word).to_ascii_uppercase();
            let flag = match key.as_str() {
                "SEEN" => Some(("\\Seen", true)),
                "UNSEEN" => Some(("\\Seen", false)),
                "DELETED" => Some(("\\Deleted", true)),
                "UNDELETED" => Some(("\\Deleted", false)),
                "FLAGGED" => Some(("\\Flagged", true)),
                "UNFLAGGED" => Some(("\\Flagged", false)),
                _ => None,
            };
            let mut argument = || words.next().map(|w| unquote(w)).unwrap_or_default();
            let mut keep_set = |set: &str, by_uid: bool| {
                let chosen = resolve_set(set, &self.keys(by_uid));
                for (i, keep) in matching.iter_mut().enumerate() {
                    *keep &= chosen.contains(&i);
                }
            };
            if let Some((flag, wanted)) = flag {
                for (keep, entry) in matching.iter_mut().zip(&entries) {
                    *keep &= has_flag(entry, flag) == wanted;
                }
                continue;
            }
            match key.as_str() {
                "CHARSET" => {
                    argument();
                }
                "FROM" => texts.push(("FROM", argument().to_lowercase())),
                "TO" => texts.push(("TO", argument().to_lowercase())),
                "SUBJECT" => texts.push(("SUBJECT", argument().to_lowercase())),
                "TEXT" | "BODY" => texts.push(("TEXT", argument().to_lowercase())),
                "UID" => keep_set(&argument(), true),
                set if set.starts_with(|c: char| c.is_ascii_digit() || c == '*') => {
                    keep_set(set, false);
                }
                _ => {}
            }
        }
        if !texts.is_empty() {
            for (i, entry) in entries.iter().enumerate() {
                if !matching[i] {
                    continue;
                }
                let Some(mail) = self.storage.get_mail(entry.id).await? else {
                    matching[i] = false;
                    continue;
                };
                matching[i] = texts.iter().all(|(field, needle)| {
                    let haystack = match *field {
                        "FROM" => mail.from.clone(),
                        "TO" => [&mail.to[..], &mail.cc, &mail.bcc].concat().join(","),
                        "SUBJECT" => mail.subject.clone(),
                        _ => mail.text.clone() + mail.html.as_deref().unwrap_or_default(),
                    };
                    haystack.to_lowercase().contains(needle)
                });
            }
        }
        Ok(self
            .keys(uid)
            .into_iter()
            .zip(matching)
            .filter_map(|(key, keep)| keep.then_some(key))
            .collect())
    }
}
