//! REST cho script/test (`/v1/...`) và DNS-over-HTTPS: JSON (`/resolve`, kiểu dns.google) và wire
//! (`/dns-query`, RFC 8484). Cổng riêng, không cần client web đã build.
//! `/v1/*` đòi admin token khi có đặt; đường truy vấn DNS và `/health` luôn mở, như một resolver.
//!
//! Thành công: `{"data": ...}`. Lỗi: `{"error": {"code", "message"}}`.
pub mod auth;
mod dns;
mod doh;
mod mail;

use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    middleware::from_fn_with_state,
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use mockhub_core::{
    config::{ApiConfig, MailConfig},
    errors::Result,
};
use mockhub_dns::Engine;
use mockhub_storage::{Storage, StorageError};
use serde::Serialize;
use serde_json::json;

/// Fault `timeout` qua HTTP: giữ request đủ lâu để client tự timeout trước.
pub(crate) const TIMEOUT_HOLD: Duration = Duration::from_secs(30);

pub(crate) type Db = State<Arc<dyn Storage>>;
pub(crate) type ApiResult = std::result::Result<Response, ApiError>;

pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub(crate) fn invalid(message: impl ToString) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid",
            message: message.to_string(),
        }
    }

    pub(crate) fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "missing or wrong admin token".into(),
        }
    }

    pub(crate) fn not_found(message: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.to_owned(),
        }
    }
}

impl From<StorageError> for ApiError {
    fn from(e: StorageError) -> Self {
        tracing::error!("api storage: {e}");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "storage",
            message: "storage error".into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}

pub(crate) fn data(value: impl Serialize) -> Response {
    Json(json!({ "data": value })).into_response()
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

/// `DELETE /v1/reset`: xoá record, fault, log truy vấn và thư, để một bộ test bắt đầu từ trống.
async fn reset(State(db): Db) -> ApiResult {
    db.clear_dns_records().await?;
    db.clear_dns_faults().await?;
    db.clear_dns_queries().await?;
    db.clear_mails().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `token`: `None` hoặc rỗng = `/v1/*` không cần xác thực.
pub fn router(engine: Engine, mail_capacity: u32, token: Option<&str>) -> Router {
    let storage = engine.storage.clone();
    let mut admin = Router::new()
        .route("/v1/reset", delete(reset))
        .merge(dns::admin_routes())
        .merge(mail::routes(mail_capacity));
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        // `route_layer`: chỉ áp cho route đã khai báo, đường lạ vẫn là 404.
        admin = admin.route_layer(from_fn_with_state(
            Arc::<str>::from(token),
            auth::require_bearer,
        ));
    }
    let queries = dns::resolve_routes()
        .merge(doh::routes())
        .with_state(engine);
    Router::new()
        .route("/health", get(health))
        .merge(admin)
        .with_state(storage)
        .merge(queries)
}

pub async fn run(
    host: &str,
    cfg: &ApiConfig,
    mail: &MailConfig,
    engine: Engine,
    token: Option<&str>,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(format!("{host}:{}", cfg.port)).await?;
    tracing::info!("api listening on http://{host}:{}", cfg.port);
    if token.is_none() {
        tracing::warn!(
            "auth.admin_token is not set: anyone who can reach mockhub can change records and read mail"
        );
    }
    axum::serve(listener, router(engine, mail.capacity, token)).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
