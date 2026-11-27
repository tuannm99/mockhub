//! Backend SQLite (file hoặc in-memory), qua sqlx. Migration nhúng sẵn vào binary.
use std::{path::Path, str::FromStr, time::Duration};

use async_trait::async_trait;
use mockhub_core::domain::{
    dns::{
        DEFAULT_TTL, DnsFault, DnsQuery, DnsRecord, FaultAction, NewDnsFault, NewDnsQuery,
        NewDnsRecord,
    },
    mail::{Mail, MailboxEntry, NewMail},
};
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteRow},
};

use crate::{
    error::{Result, StorageError},
    repo::{DnsFaultRepo, DnsQueryRepo, DnsRecordRepo, MailRepo, MailboxRepo},
};

pub struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    /// `url`: `sqlite://data/mockhub.db` (relative), `sqlite:///data/mockhub.db` (absolute),
    /// hoặc `sqlite::memory:`. File + thư mục cha được tạo nếu chưa có; migration chạy luôn.
    pub async fn connect(url: &str) -> Result<Self> {
        let in_memory = url.contains(":memory:");
        let opts = SqliteConnectOptions::from_str(url)
            .map_err(StorageError::backend)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);

        if !in_memory && let Some(dir) = Path::new(opts.get_filename()).parent() {
            std::fs::create_dir_all(dir)?;
        }

        // In-memory: mỗi connection là một DB riêng, nên ép đúng 1 connection.
        let pool = SqlitePoolOptions::new()
            .max_connections(if in_memory { 1 } else { 5 })
            .connect_with(opts)
            .await
            .map_err(StorageError::backend)?;

        sqlx::migrate!("migrations/sqlite")
            .run(&pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(Self { pool })
    }
}

fn dns_from_row(r: &SqliteRow) -> DnsRecord {
    DnsRecord {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        value: r.get("value"),
        ttl: r.get("ttl"),
    }
}

fn fault_from_row(r: &SqliteRow) -> Result<DnsFault> {
    let action: String = r.get("action");
    Ok(DnsFault {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        action: FaultAction::parse(&action).ok_or_else(|| {
            StorageError::backend(std::io::Error::other(format!(
                "unknown fault action {action:?}"
            )))
        })?,
        delay_ms: r.get::<i64, _>("delay_ms").max(0) as u64,
    })
}

#[async_trait]
impl DnsRecordRepo for SqliteStorage {
    async fn list_dns_records(&self) -> Result<Vec<DnsRecord>> {
        let rows = sqlx::query("SELECT id, name, kind, value, ttl FROM dns_records ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(rows.iter().map(dns_from_row).collect())
    }

    async fn create_dns_record(&self, new: NewDnsRecord) -> Result<DnsRecord> {
        let row = sqlx::query(
            "INSERT INTO dns_records (name, kind, value, ttl) VALUES (?, ?, ?, ?) \
             RETURNING id, name, kind, value, ttl",
        )
        .bind(&new.name)
        .bind(&new.kind)
        .bind(&new.value)
        .bind(new.ttl.unwrap_or(DEFAULT_TTL))
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(dns_from_row(&row))
    }

    async fn delete_dns_record(&self, id: i64) -> Result<bool> {
        let done = sqlx::query("DELETE FROM dns_records WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(done.rows_affected() > 0)
    }

    async fn delete_dns_records_named(&self, name: &str, kind: Option<&str>) -> Result<u64> {
        let done =
            sqlx::query("DELETE FROM dns_records WHERE name = ?1 AND (?2 IS NULL OR kind = ?2)")
                .bind(name)
                .bind(kind)
                .execute(&self.pool)
                .await
                .map_err(StorageError::backend)?;
        Ok(done.rows_affected())
    }

    async fn clear_dns_records(&self) -> Result<()> {
        sqlx::query("DELETE FROM dns_records")
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }
}

#[async_trait]
impl DnsFaultRepo for SqliteStorage {
    async fn list_dns_faults(&self) -> Result<Vec<DnsFault>> {
        let rows =
            sqlx::query("SELECT id, name, kind, action, delay_ms FROM dns_faults ORDER BY id")
                .fetch_all(&self.pool)
                .await
                .map_err(StorageError::backend)?;
        rows.iter().map(fault_from_row).collect()
    }

    async fn create_dns_fault(&self, new: NewDnsFault) -> Result<DnsFault> {
        let row = sqlx::query(
            "INSERT INTO dns_faults (name, kind, action, delay_ms) VALUES (?, ?, ?, ?) \
             RETURNING id, name, kind, action, delay_ms",
        )
        .bind(&new.name)
        .bind(&new.kind)
        .bind(new.action.as_str())
        .bind(i64::try_from(new.delay_ms).unwrap_or(i64::MAX))
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        fault_from_row(&row)
    }

    async fn delete_dns_fault(&self, id: i64) -> Result<bool> {
        let done = sqlx::query("DELETE FROM dns_faults WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(done.rows_affected() > 0)
    }

    async fn clear_dns_faults(&self) -> Result<()> {
        sqlx::query("DELETE FROM dns_faults")
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }
}

const MAIL_COLUMNS: &str = "id, envelope_from, subject, text_body, html_body, raw, received_at";

impl SqliteStorage {
    async fn mail_from_row(&self, r: &SqliteRow) -> Result<Mail> {
        let id: i64 = r.get("id");
        let recipients =
            sqlx::query("SELECT kind, address FROM mail_recipients WHERE mail_id = ? ORDER BY id")
                .bind(id)
                .fetch_all(&self.pool)
                .await
                .map_err(StorageError::backend)?;
        let of = |kind: &str| -> Vec<String> {
            recipients
                .iter()
                .filter(|x| x.get::<String, _>("kind") == kind)
                .map(|x| x.get("address"))
                .collect()
        };
        Ok(Mail {
            id,
            received_at: r.get("received_at"),
            from: r.get("envelope_from"),
            to: of("to"),
            cc: of("cc"),
            bcc: of("bcc"),
            subject: r.get("subject"),
            text: r.get("text_body"),
            html: r.get("html_body"),
            raw: String::from_utf8_lossy(&r.get::<Vec<u8>, _>("raw")).into_owned(),
        })
    }
}

#[async_trait]
impl MailRepo for SqliteStorage {
    async fn save_mail(&self, new: NewMail, keep: u32) -> Result<Mail> {
        let mut tx = self.pool.begin().await.map_err(StorageError::backend)?;
        let id: i64 = sqlx::query(
            "INSERT INTO mails (envelope_from, subject, text_body, html_body, raw) \
             VALUES (?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(&new.from)
        .bind(&new.subject)
        .bind(&new.text)
        .bind(&new.html)
        .bind(&new.raw)
        .fetch_one(&mut *tx)
        .await
        .map_err(StorageError::backend)?
        .get("id");
        for (kind, addresses) in [("to", &new.to), ("cc", &new.cc), ("bcc", &new.bcc)] {
            for address in addresses {
                sqlx::query(
                    "INSERT INTO mail_recipients (mail_id, kind, address) VALUES (?, ?, ?)",
                )
                .bind(id)
                .bind(kind)
                .bind(address)
                .execute(&mut *tx)
                .await
                .map_err(StorageError::backend)?;
            }
        }
        sqlx::query(
            "DELETE FROM mails WHERE id NOT IN (SELECT id FROM mails ORDER BY id DESC LIMIT ?)",
        )
        .bind(keep.max(1))
        .execute(&mut *tx)
        .await
        .map_err(StorageError::backend)?;
        tx.commit().await.map_err(StorageError::backend)?;
        self.get_mail(id)
            .await?
            .ok_or_else(|| StorageError::backend(std::io::Error::other("saved mail is gone")))
    }

    async fn list_mails(&self, recipient: Option<&str>, limit: u32) -> Result<Vec<Mail>> {
        let sql = format!(
            "SELECT {MAIL_COLUMNS} FROM mails m WHERE ?1 IS NULL OR EXISTS \
             (SELECT 1 FROM mail_recipients r WHERE r.mail_id = m.id AND r.address = ?1 COLLATE NOCASE) \
             ORDER BY id DESC LIMIT ?2"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(recipient)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            out.push(self.mail_from_row(row).await?);
        }
        Ok(out)
    }

    async fn get_mail(&self, id: i64) -> Result<Option<Mail>> {
        let sql = format!("SELECT {MAIL_COLUMNS} FROM mails WHERE id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        match row {
            Some(row) => Ok(Some(self.mail_from_row(&row).await?)),
            None => Ok(None),
        }
    }

    async fn clear_mails(&self) -> Result<()> {
        sqlx::query("DELETE FROM mails")
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn get_mail_raw(&self, id: i64) -> Result<Option<Vec<u8>>> {
        let row = sqlx::query("SELECT raw FROM mails WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(row.map(|r| r.get("raw")))
    }

    async fn next_mail_id(&self) -> Result<i64> {
        // `sqlite_sequence` giữ id lớn nhất từng cấp, kể cả khi thư đó đã xoá.
        let row = sqlx::query(
            "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'mails'), 0) + 1 AS next",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.get("next"))
    }
}

fn mailbox_key(mailbox: &str) -> String {
    mailbox.trim().to_lowercase()
}

#[async_trait]
impl MailboxRepo for SqliteStorage {
    async fn list_mailbox(&self, mailbox: &str) -> Result<Vec<MailboxEntry>> {
        let key = mailbox_key(mailbox);
        let rows = sqlx::query(
            "SELECT m.id, length(m.raw) AS size FROM mails m \
             WHERE (?1 = '*' OR EXISTS (SELECT 1 FROM mail_recipients r \
                    WHERE r.mail_id = m.id AND r.address = ?1 COLLATE NOCASE)) \
             AND NOT EXISTS (SELECT 1 FROM mailbox_hidden h WHERE h.mailbox = ?1 AND h.mail_id = m.id) \
             ORDER BY m.id",
        )
        .bind(&key)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let flags =
            sqlx::query("SELECT mail_id, flag FROM mailbox_flags WHERE mailbox = ? ORDER BY flag")
                .bind(&key)
                .fetch_all(&self.pool)
                .await
                .map_err(StorageError::backend)?;
        let mut by_mail: std::collections::HashMap<i64, Vec<String>> = Default::default();
        for row in &flags {
            by_mail
                .entry(row.get("mail_id"))
                .or_default()
                .push(row.get("flag"));
        }
        Ok(rows
            .iter()
            .map(|r| {
                let id: i64 = r.get("id");
                MailboxEntry {
                    id,
                    size: r.get::<i64, _>("size").max(0) as u64,
                    flags: by_mail.remove(&id).unwrap_or_default(),
                }
            })
            .collect())
    }

    async fn mailbox_flags(&self, mailbox: &str, id: i64) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT flag FROM mailbox_flags WHERE mailbox = ? AND mail_id = ? ORDER BY flag",
        )
        .bind(mailbox_key(mailbox))
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(rows.iter().map(|r| r.get("flag")).collect())
    }

    async fn mails_flagged(&self, mailbox: &str, flag: &str) -> Result<Vec<i64>> {
        let rows = sqlx::query("SELECT mail_id FROM mailbox_flags WHERE mailbox = ? AND flag = ?")
            .bind(mailbox_key(mailbox))
            .bind(flag)
            .fetch_all(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(rows.iter().map(|r| r.get("mail_id")).collect())
    }

    async fn set_mailbox_flag(&self, mailbox: &str, id: i64, flag: &str, on: bool) -> Result<()> {
        let sql = if on {
            "INSERT OR IGNORE INTO mailbox_flags (mailbox, mail_id, flag) \
             SELECT ?1, id, ?3 FROM mails WHERE id = ?2"
        } else {
            "DELETE FROM mailbox_flags WHERE mailbox = ?1 AND mail_id = ?2 AND flag = ?3"
        };
        sqlx::query(sql)
            .bind(mailbox_key(mailbox))
            .bind(id)
            .bind(flag)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn hide_from_mailbox(&self, mailbox: &str, ids: &[i64]) -> Result<()> {
        let key = mailbox_key(mailbox);
        let mut tx = self.pool.begin().await.map_err(StorageError::backend)?;
        for id in ids {
            sqlx::query(
                "INSERT OR IGNORE INTO mailbox_hidden (mailbox, mail_id) \
                 SELECT ?1, id FROM mails WHERE id = ?2",
            )
            .bind(&key)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(StorageError::backend)?;
        }
        tx.commit().await.map_err(StorageError::backend)?;
        Ok(())
    }
}

#[async_trait]
impl DnsQueryRepo for SqliteStorage {
    async fn log_dns_query(&self, new: NewDnsQuery, keep: u32) -> Result<()> {
        let id: i64 = sqlx::query(
            "INSERT INTO dns_queries (proto, name, kind, rcode, answers, source) \
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(&new.proto)
        .bind(&new.name)
        .bind(&new.kind)
        .bind(&new.rcode)
        .bind(new.answers)
        .bind(&new.source)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?
        .get("id");
        sqlx::query("DELETE FROM dns_queries WHERE id <= ?")
            .bind(id - i64::from(keep))
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn list_dns_queries(&self, limit: u32) -> Result<Vec<DnsQuery>> {
        let rows = sqlx::query(
            "SELECT id, time, proto, name, kind, rcode, answers, source FROM dns_queries \
             ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(rows
            .iter()
            .map(|r| DnsQuery {
                id: r.get("id"),
                time: r.get("time"),
                entry: NewDnsQuery {
                    proto: r.get("proto"),
                    name: r.get("name"),
                    kind: r.get("kind"),
                    rcode: r.get("rcode"),
                    answers: r.get("answers"),
                    source: r.get("source"),
                },
            })
            .collect())
    }

    async fn clear_dns_queries(&self) -> Result<()> {
        sqlx::query("DELETE FROM dns_queries")
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, kind: &str, value: &str) -> NewDnsRecord {
        NewDnsRecord {
            name: name.into(),
            kind: kind.into(),
            value: value.into(),
            ttl: None,
        }
    }

    #[tokio::test]
    async fn dns_crud() {
        let s = SqliteStorage::connect("sqlite::memory:").await.unwrap();
        assert!(s.list_dns_records().await.unwrap().is_empty());

        let a = s
            .create_dns_record(record("app.test", "A", "127.0.0.1"))
            .await
            .unwrap();
        assert_eq!(a.ttl, DEFAULT_TTL);
        let b = s
            .create_dns_record(record("www.app.test", "CNAME", "app.test"))
            .await
            .unwrap();
        assert!(b.id > a.id);
        assert_eq!(
            s.list_dns_records().await.unwrap(),
            vec![a.clone(), b.clone()]
        );

        assert!(s.delete_dns_record(a.id).await.unwrap());
        assert!(
            !s.delete_dns_record(a.id).await.unwrap(),
            "xoá lần hai không phải lỗi"
        );
        assert_eq!(s.list_dns_records().await.unwrap(), vec![b]);

        assert_eq!(
            s.delete_dns_records_named("www.app.test", Some("A"))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            s.delete_dns_records_named("www.app.test", None)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn mails_filter_by_any_recipient_and_keep_the_newest() {
        let s = SqliteStorage::connect("sqlite::memory:").await.unwrap();
        let mail = |n: u32| NewMail {
            from: "app@mock.test".into(),
            to: vec![format!("to{n}@mock.test")],
            bcc: vec![format!("Hidden{n}@mock.test")],
            subject: format!("s{n}"),
            raw: b"raw".to_vec(),
            ..Default::default()
        };
        for n in 1..=3 {
            s.save_mail(mail(n), 2).await.unwrap();
        }
        let all = s.list_mails(None, 10).await.unwrap();
        assert_eq!(
            all.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(),
            ["s3", "s2"]
        );
        let hidden = s.list_mails(Some("hidden2@MOCK.test"), 10).await.unwrap();
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].bcc, ["Hidden2@mock.test"]);
        assert!(hidden[0].received_at.ends_with('Z'));
        s.clear_mails().await.unwrap();
        assert!(s.list_mails(None, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_mailbox_has_its_own_flags_and_deletions() {
        let s = SqliteStorage::connect("sqlite::memory:").await.unwrap();
        assert_eq!(s.next_mail_id().await.unwrap(), 1);
        let mail = NewMail {
            to: vec!["Bob@mock.test".into()],
            bcc: vec!["eve@mock.test".into()],
            raw: b"12345".to_vec(),
            ..Default::default()
        };
        let id = s.save_mail(mail, 10).await.unwrap().id;
        s.set_mailbox_flag("BOB@mock.test", id, "\\Seen", true)
            .await
            .unwrap();
        s.set_mailbox_flag("bob@mock.test", id, "\\seen", true)
            .await
            .unwrap();
        s.set_mailbox_flag("bob@mock.test", 999, "\\Seen", true)
            .await
            .unwrap();
        let bob = s.list_mailbox("bob@mock.test").await.unwrap();
        assert_eq!((bob[0].size, bob[0].flags.len()), (5, 1));
        assert!(
            s.list_mailbox("eve@mock.test").await.unwrap()[0]
                .flags
                .is_empty()
        );
        s.hide_from_mailbox("bob@mock.test", &[id]).await.unwrap();
        assert!(s.list_mailbox("bob@mock.test").await.unwrap().is_empty());
        assert_eq!(s.list_mailbox("*").await.unwrap().len(), 1);
        assert_eq!(s.list_mails(None, 10).await.unwrap().len(), 1);
        s.clear_mails().await.unwrap();
        assert_eq!(
            s.next_mail_id().await.unwrap(),
            id + 1,
            "ids are not reused"
        );
    }

    #[tokio::test]
    async fn the_query_log_keeps_the_newest() {
        let s = SqliteStorage::connect("sqlite::memory:").await.unwrap();
        for n in 1..=4 {
            let entry = NewDnsQuery {
                proto: "udp".into(),
                name: format!("n{n}.test"),
                kind: "A".into(),
                rcode: "NOERROR".into(),
                answers: 0,
                source: "mock".into(),
            };
            s.log_dns_query(entry, 2).await.unwrap();
        }
        let names: Vec<_> = s
            .list_dns_queries(10)
            .await
            .unwrap()
            .into_iter()
            .map(|q| q.entry.name)
            .collect();
        assert_eq!(names, ["n4.test", "n3.test"]);
    }
}
