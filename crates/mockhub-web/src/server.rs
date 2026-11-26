//! Chỉ phía server: giữ `Storage` dùng chung cho các server function (`api.rs`).
//! `mockhub-server` gọi [`set_storage`] lúc khởi động; handler lấy lại bằng [`storage`].
use std::sync::{Arc, OnceLock};

use mockhub_storage::Storage;

static STORAGE: OnceLock<Arc<dyn Storage>> = OnceLock::new();

/// Đăng ký storage (gọi một lần lúc khởi động; lần hai bị bỏ qua).
pub fn set_storage(s: Arc<dyn Storage>) {
    let _ = STORAGE.set(s);
}

pub(crate) fn storage() -> Result<&'static Arc<dyn Storage>, ServerError> {
    STORAGE.get().ok_or(ServerError::StorageNotInitialized)
}

#[derive(Debug)]
pub enum ServerError {
    StorageNotInitialized,
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StorageNotInitialized => f.write_str("storage chưa được khởi tạo (gọi set_storage lúc khởi động)"),
        }
    }
}

impl std::error::Error for ServerError {}

/// Dev độc lập (`dx serve --fullstack`, không qua `mockhub-server`): tự mở storage từ env
/// `MOCKHUB__DATABASE__URL`. Mặc định là `data/mockhub.db` ở **root repo** (neo theo vị trí crate
/// lúc compile), vì `dx serve` chạy với cwd = `crates/mockhub-web` nên đường dẫn tương đối sẽ
/// tạo DB nhầm chỗ.
pub async fn init_from_env() -> mockhub_storage::Result<()> {
    let url = std::env::var("MOCKHUB__DATABASE__URL").unwrap_or_else(|_| {
        format!("sqlite://{}/../../data/mockhub.db", env!("CARGO_MANIFEST_DIR"))
    });
    set_storage(mockhub_storage::connect(&url).await?);
    Ok(())
}
