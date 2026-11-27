//! Nạp record/fault ban đầu từ file JSON, chỉ khi DB chưa có record nào.
use mockhub_core::domain::dns::{NewDnsFault, NewDnsRecord};
use mockhub_storage::Storage;
use serde::Deserialize;

use crate::records::{prepare, prepare_fault};

#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error("đọc file seed {0}: {1}")]
    Read(String, std::io::Error),
    #[error("file seed không đúng JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error(transparent)]
    Record(#[from] crate::records::RecordError),
    #[error(transparent)]
    Storage(#[from] mockhub_storage::StorageError),
}

#[derive(Debug, Default, Deserialize)]
struct Seed {
    #[serde(default)]
    records: Vec<NewDnsRecord>,
    #[serde(default)]
    faults: Vec<NewDnsFault>,
}

/// Trả số record đã nạp. File sai thì không nạp gì.
pub async fn load(storage: &dyn Storage, path: &str) -> Result<usize, SeedError> {
    if !storage.list_dns_records().await?.is_empty() {
        return Ok(0);
    }
    let text = std::fs::read_to_string(path).map_err(|e| SeedError::Read(path.to_owned(), e))?;
    let seed: Seed = serde_json::from_str(&text)?;
    let mut records = Vec::new();
    let mut planned = Vec::new();
    for new in seed.records {
        let record = prepare(new, &planned, false)?;
        planned.push(mockhub_core::domain::dns::DnsRecord {
            id: 0,
            name: record.name.clone(),
            kind: record.kind.clone(),
            value: record.value.clone(),
            ttl: 0,
        });
        records.push(record);
    }
    let faults = seed
        .faults
        .into_iter()
        .map(prepare_fault)
        .collect::<Result<Vec<_>, _>>()?;
    let count = records.len();
    for record in records {
        storage.create_dns_record(record).await?;
    }
    storage.clear_dns_faults().await?;
    for fault in faults {
        storage.create_dns_fault(fault).await?;
    }
    Ok(count)
}
