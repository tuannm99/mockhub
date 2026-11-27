use std::{str::FromStr, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{delete, get},
};
use hickory_proto::rr::{RData, Record, RecordType};
use mockhub_core::domain::dns::{NewDnsFault, NewDnsRecord};
use mockhub_dns::{Engine, Outcome, QUERY_LOG_LIMIT, records};
use mockhub_storage::Storage;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{ApiError, ApiResult, Db, TIMEOUT_HOLD, data};

pub(crate) fn resolve_routes() -> Router<Engine> {
    Router::new().route("/resolve", get(resolve))
}

pub(crate) fn admin_routes() -> Router<Arc<dyn Storage>> {
    Router::new()
        .route("/v1/dns/queries", get(list_queries).delete(clear_queries))
        .route(
            "/v1/dns/records",
            get(list_records).post(add_record).delete(delete_named),
        )
        .route("/v1/dns/records/{id}", delete(delete_record))
        .route(
            "/v1/dns/faults",
            get(list_faults).post(add_fault).delete(clear_faults),
        )
        .route("/v1/dns/faults/{id}", delete(delete_fault))
}

#[derive(Deserialize)]
struct Question {
    name: Option<String>,
    r#type: Option<String>,
}

/// `data` của một record, dạng văn bản như dns.google: TXT nằm trong ngoặc kép.
fn presentation(record: &Record) -> String {
    match &record.data {
        RData::TXT(txt) => txt
            .txt_data
            .iter()
            .map(|chunk| {
                let text = String::from_utf8_lossy(chunk)
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"");
                format!("\"{text}\"")
            })
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    }
}

async fn resolve(State(engine): State<Engine>, Query(question): Query<Question>) -> ApiResult {
    let name = question
        .name
        .filter(|n| !n.is_empty())
        .ok_or_else(|| ApiError::invalid("name is required"))?;
    let wanted = question.r#type.unwrap_or_else(|| "A".to_owned());
    // Nhận cả tên (`TXT`) lẫn số (`16`).
    let qtype = match wanted.parse::<u16>() {
        Ok(number) => RecordType::from(number),
        Err(_) => RecordType::from_str(&wanted.to_ascii_uppercase())
            .map_err(|_| ApiError::invalid(format!("unknown type {wanted}")))?,
    };
    let Outcome::Reply(answer) = engine.lookup("doh-json", &name, qtype).await else {
        tokio::time::sleep(TIMEOUT_HOLD).await;
        return Ok(StatusCode::GATEWAY_TIMEOUT.into_response());
    };
    let mut body = json!({
        "Status": u16::from(answer.code),
        "TC": false, "RD": true, "RA": true, "AD": false, "CD": false,
        "Question": [{"name": format!("{}.", name.trim_end_matches('.')), "type": u16::from(qtype)}],
    });
    if !answer.records.is_empty() {
        body["Answer"] = answer
            .records
            .iter()
            .map(|r| {
                json!({"name": r.name.to_ascii(), "type": u16::from(r.record_type()), "TTL": r.ttl, "data": presentation(r)})
            })
            .collect::<Vec<Value>>()
            .into();
    }
    Ok((
        [(header::CONTENT_TYPE, "application/dns-json")],
        body.to_string(),
    )
        .into_response())
}

#[derive(Deserialize)]
struct AddRecord {
    #[serde(flatten)]
    record: NewDnsRecord,
    /// Thay mọi record cùng tên + loại (hợp cho TXT token).
    #[serde(default)]
    replace: bool,
}

fn body<T>(parsed: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    parsed
        .map(|Json(v)| v)
        .map_err(|e| ApiError::invalid(e.body_text()))
}

async fn list_records(State(db): Db) -> ApiResult {
    Ok(data(db.list_dns_records().await?))
}

async fn add_record(State(db): Db, parsed: Result<Json<AddRecord>, JsonRejection>) -> ApiResult {
    let AddRecord { record, replace } = body(parsed)?;
    let existing = db.list_dns_records().await?;
    let record = records::prepare(record, &existing, replace).map_err(ApiError::invalid)?;
    if replace {
        db.delete_dns_records_named(&record.name, Some(&record.kind))
            .await?;
    }
    Ok((
        StatusCode::CREATED,
        data(db.create_dns_record(record).await?),
    )
        .into_response())
}

#[derive(Deserialize)]
struct Named {
    name: Option<String>,
    #[serde(alias = "type")]
    kind: Option<String>,
}

async fn delete_named(State(db): Db, Query(q): Query<Named>) -> ApiResult {
    // Không có `name` thì từ chối: xoá sạch phải gọi `/v1/reset`.
    let name = q.name.ok_or_else(|| {
        ApiError::invalid("name is required (DELETE /v1/reset clears everything)")
    })?;
    let name = records::normalize_name(&name).map_err(ApiError::invalid)?;
    let kind = q
        .kind
        .map(|k| records::parse_kind(&k))
        .transpose()
        .map_err(ApiError::invalid)?;
    let kind = kind.map(|k| k.to_string());
    let deleted = db.delete_dns_records_named(&name, kind.as_deref()).await?;
    Ok(data(json!({ "deleted": deleted })))
}

async fn delete_record(State(db): Db, Path(id): Path<i64>) -> ApiResult {
    if !db.delete_dns_record(id).await? {
        return Err(ApiError::not_found("No such record."));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn list_faults(State(db): Db) -> ApiResult {
    Ok(data(db.list_dns_faults().await?))
}

async fn add_fault(State(db): Db, parsed: Result<Json<NewDnsFault>, JsonRejection>) -> ApiResult {
    let fault = records::prepare_fault(body(parsed)?).map_err(ApiError::invalid)?;
    Ok((StatusCode::CREATED, data(db.create_dns_fault(fault).await?)).into_response())
}

async fn delete_fault(State(db): Db, Path(id): Path<i64>) -> ApiResult {
    if !db.delete_dns_fault(id).await? {
        return Err(ApiError::not_found("No such fault."));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn clear_faults(State(db): Db) -> ApiResult {
    db.clear_dns_faults().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
struct Page {
    limit: Option<u32>,
}

/// Mới nhất trước.
async fn list_queries(State(db): Db, Query(page): Query<Page>) -> ApiResult {
    let limit = page.limit.unwrap_or(QUERY_LOG_LIMIT).min(QUERY_LOG_LIMIT);
    Ok(data(db.list_dns_queries(limit).await?))
}

async fn clear_queries(State(db): Db) -> ApiResult {
    db.clear_dns_queries().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
