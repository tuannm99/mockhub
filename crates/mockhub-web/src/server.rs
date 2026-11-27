//! Chỉ phía server: giữ [`Engine`] (storage + DNS) dùng chung cho các server function (`api.rs`).
//! `mockhub-server` gọi [`init`] lúc khởi động; handler lấy lại bằng [`engine`] / [`storage`].
use std::sync::{Arc, OnceLock};

use mockhub_dns::Engine;
use mockhub_storage::Storage;

use crate::api::Ports;

static STATE: OnceLock<(Engine, Ports)> = OnceLock::new();

/// Gọi một lần lúc khởi động; lần hai bị bỏ qua.
pub fn init(engine: Engine, ports: Ports) {
    let _ = STATE.set((engine, ports));
}

fn state() -> Result<&'static (Engine, Ports), ServerError> {
    STATE.get().ok_or(ServerError::NotInitialized)
}

pub(crate) fn engine() -> Result<&'static Engine, ServerError> {
    Ok(&state()?.0)
}

pub(crate) fn ports() -> Result<Ports, ServerError> {
    Ok(state()?.1)
}

pub(crate) fn storage() -> Result<&'static Arc<dyn Storage>, ServerError> {
    Ok(&engine()?.storage)
}

#[derive(Debug)]
pub enum ServerError {
    NotInitialized,
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInitialized => {
                f.write_str("server chưa được khởi tạo (gọi init lúc khởi động)")
            }
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
        format!(
            "sqlite://{}/../../data/mockhub.db",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let engine = Engine::new(mockhub_storage::connect(&url).await?);
    init(engine, Ports::default());
    Ok(())
}
