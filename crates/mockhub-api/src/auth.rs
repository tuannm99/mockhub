//! Xác thực bằng admin token. Không đặt token thì mọi thứ mở. Token đi dạng văn bản qua HTTP:
//! instance dùng chung nên đứng sau TLS.
use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use mockhub_core::auth::{basic_matches, bearer_matches};

use crate::ApiError;

fn authorization(request: &Request) -> &str {
    request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
}

/// REST: `Authorization: Bearer <token>`.
pub(crate) async fn require_bearer(
    State(token): State<Arc<str>>,
    request: Request,
    next: Next,
) -> Response {
    if bearer_matches(&token, authorization(&request)) {
        next.run(request).await
    } else {
        ApiError::unauthorized().into_response()
    }
}

/// Request ghi dữ liệu có `Origin` của trang khác không. Trình duyệt tự gửi kèm Basic đã nhớ,
/// nên chỉ đúng mật khẩu là chưa đủ.
fn cross_site(request: &Request) -> bool {
    if matches!(*request.method(), Method::GET | Method::HEAD) {
        return false;
    }
    let header = |name| request.headers().get(name).and_then(|v| v.to_str().ok());
    header(header::ORIGIN).is_some_and(|origin| {
        let authority = origin.split_once("://").map(|(_, rest)| rest);
        authority.is_none() || authority != header(header::HOST)
    })
}

/// Web UI (trang và server function): HTTP Basic, tên bất kỳ, mật khẩu = token. Trình duyệt tự
/// hỏi và tự gửi lại.
pub async fn require_basic(
    State(token): State<Arc<str>>,
    request: Request,
    next: Next,
) -> Response {
    if !basic_matches(&token, authorization(&request)) {
        return (
            StatusCode::UNAUTHORIZED,
            [(
                header::WWW_AUTHENTICATE,
                r#"Basic realm="mockhub", charset="UTF-8""#,
            )],
            "mockhub: nhập admin token làm mật khẩu (tên bất kỳ).",
        )
            .into_response();
    }
    if cross_site(&request) {
        return (StatusCode::FORBIDDEN, "cross-site request refused").into_response();
    }
    next.run(request).await
}
