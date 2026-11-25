//! Backend SQLite (file hoặc in-memory), qua sqlx. Migration nhúng sẵn vào binary.
use std::{path::Path, str::FromStr, time::Duration};

use async_trait::async_trait;
use mockhub_core::domain::dns::{DnsRecord, NewDnsRecord};
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteRow},
};

use crate::{
    error::{Result, StorageError},
    repo::DnsRecordRepo,
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
    }
}

#[async_trait]
impl DnsRecordRepo for SqliteStorage {
    async fn list(&self) -> Result<Vec<DnsRecord>> {
        let rows = sqlx::query("SELECT id, name, kind, value FROM dns_records ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(rows.iter().map(dns_from_row).collect())
    }

    async fn create(&self, new: NewDnsRecord) -> Result<DnsRecord> {
        let row = sqlx::query(
            "INSERT INTO dns_records (name, kind, value) VALUES (?, ?, ?) \
             RETURNING id, name, kind, value",
        )
        .bind(&new.name)
        .bind(&new.kind)
        .bind(&new.value)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(dns_from_row(&row))
    }

    async fn delete(&self, id: i64) -> Result<bool> {
        let done = sqlx::query("DELETE FROM dns_records WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(done.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dns_crud() {
        let s = SqliteStorage::connect("sqlite::memory:").await.unwrap();
        assert!(s.list().await.unwrap().is_empty());

        let a = s
            .create(NewDnsRecord { name: "app.test".into(), kind: "A".into(), value: "127.0.0.1".into() })
            .await
            .unwrap();
        let b = s
            .create(NewDnsRecord { name: "www.app.test".into(), kind: "CNAME".into(), value: "app.test".into() })
            .await
            .unwrap();
        assert!(b.id > a.id);
        assert_eq!(s.list().await.unwrap(), vec![a.clone(), b.clone()]);

        assert!(s.delete(a.id).await.unwrap());
        assert!(!s.delete(a.id).await.unwrap(), "xoá lần hai không phải lỗi");
        assert_eq!(s.list().await.unwrap(), vec![b]);
    }
}
