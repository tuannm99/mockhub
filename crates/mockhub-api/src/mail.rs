use std::sync::Arc;

use axum::{
    Extension, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use mockhub_storage::Storage;
use serde::Deserialize;

use crate::{ApiError, ApiResult, Db, data};

#[derive(Clone, Copy)]
struct Capacity(u32);

pub(crate) fn routes(capacity: u32) -> Router<Arc<dyn Storage>> {
    Router::new()
        .route("/v1/mail/messages", get(list).delete(clear))
        .route("/v1/mail/messages/latest", get(latest))
        .route("/v1/mail/messages/{id}", get(one))
        .layer(Extension(Capacity(capacity)))
}

#[derive(Deserialize)]
struct Filter {
    /// Địa chỉ nhận (to, cc hoặc bcc), không phân biệt hoa thường.
    to: Option<String>,
    limit: Option<u32>,
}

fn recipient(filter: &Filter) -> Option<&str> {
    filter
        .to
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Mới nhất trước.
async fn list(
    State(db): Db,
    Extension(Capacity(max)): Extension<Capacity>,
    Query(f): Query<Filter>,
) -> ApiResult {
    let limit = f.limit.unwrap_or(max).min(max);
    Ok(data(db.list_mails(recipient(&f), limit).await?))
}

async fn latest(State(db): Db, Query(f): Query<Filter>) -> ApiResult {
    let mail = db.list_mails(recipient(&f), 1).await?.into_iter().next();
    Ok(data(
        mail.ok_or_else(|| ApiError::not_found("No message yet."))?,
    ))
}

async fn one(State(db): Db, Path(id): Path<i64>) -> ApiResult {
    Ok(data(
        db.get_mail(id)
            .await?
            .ok_or_else(|| ApiError::not_found("No such message."))?,
    ))
}

async fn clear(State(db): Db) -> ApiResult {
    db.clear_mails().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
