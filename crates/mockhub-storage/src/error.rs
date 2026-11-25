/// Lỗi storage, **không lộ kiểu của backend** (sqlx...) để đổi backend không đụng caller.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database url không được hỗ trợ: {0} (hiện chỉ có `sqlite:`)")]
    UnsupportedUrl(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("database: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl StorageError {
    /// Bọc lỗi của backend. Mỗi backend gọi hàm này thay vì thêm variant riêng.
    pub fn backend(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Backend(Box::new(e))
    }
}

pub type Result<T> = std::result::Result<T, StorageError>;
