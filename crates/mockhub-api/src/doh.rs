//! DNS-over-HTTPS dạng wire (RFC 8484): `GET /dns-query?dns=<base64url>` hoặc `POST` với
//! `content-type: application/dns-message`.
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hickory_proto::op::{Message, MessageType};
use mockhub_dns::{Engine, server::answer_message};
use serde::Deserialize;

use crate::{ApiError, TIMEOUT_HOLD};

const CONTENT_TYPE: &str = "application/dns-message";
const MAX_MESSAGE: usize = 65_535;

pub(crate) fn routes() -> Router<Engine> {
    Router::new().route(
        "/dns-query",
        get(by_get)
            .post(by_post)
            .layer(DefaultBodyLimit::max(MAX_MESSAGE)),
    )
}

#[derive(Deserialize)]
struct Params {
    dns: Option<String>,
}

fn malformed() -> ApiError {
    ApiError::invalid("malformed DNS query")
}

async fn by_get(State(engine): State<Engine>, Query(params): Query<Params>) -> Response {
    let decoded = params
        .dns
        .and_then(|d| URL_SAFE_NO_PAD.decode(d.trim_end_matches('=')).ok());
    match decoded {
        Some(packet) => answer(&engine, &packet).await,
        None => malformed().into_response(),
    }
}

async fn by_post(State(engine): State<Engine>, headers: HeaderMap, body: Bytes) -> Response {
    let sent = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !sent.eq_ignore_ascii_case(CONTENT_TYPE) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    answer(&engine, &body).await
}

async fn answer(engine: &Engine, packet: &[u8]) -> Response {
    let query = match Message::from_vec(packet) {
        Ok(q) if q.metadata.message_type == MessageType::Query => q,
        _ => return malformed().into_response(),
    };
    let Some(response) = answer_message(engine, "doh", &query).await else {
        // Fault `timeout`: giữ request để client tự timeout trước.
        tokio::time::sleep(TIMEOUT_HOLD).await;
        return StatusCode::GATEWAY_TIMEOUT.into_response();
    };
    match response.to_vec() {
        Ok(bytes) => ([(header::CONTENT_TYPE, CONTENT_TYPE)], bytes).into_response(),
        Err(e) => {
            tracing::error!("doh encode: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
