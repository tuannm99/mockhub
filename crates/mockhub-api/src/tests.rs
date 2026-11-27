use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hickory_proto::{
    op::{Message, MessageType, OpCode, Query},
    rr::{Name, RecordType},
};
use mockhub_core::domain::mail::NewMail;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;

async fn app() -> (Router, Arc<dyn Storage>) {
    let storage = mockhub_storage::connect("sqlite::memory:").await.unwrap();
    (router(Engine::new(storage.clone()), 500, None), storage)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let body = body.map_or_else(Body::empty, |v| Body::from(v.to_string()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn a_record_added_over_rest_is_answered_over_doh_json() {
    let (app, _) = app().await;
    let add = json!({"name": "_WAF-Verify.Shop.test.", "type": "txt", "value": "waf-verify-1"});
    let (status, made) = call(&app, "POST", "/v1/dns/records", Some(add)).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(made["data"]["name"], "_waf-verify.shop.test");

    let (status, got) = call(
        &app,
        "GET",
        "/resolve?name=_waf-verify.shop.test&type=TXT",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["Status"], 0);
    assert_eq!(got["Answer"][0]["data"], "\"waf-verify-1\"");
    assert_eq!(got["Answer"][0]["type"], 16);

    // replace: đúng một giá trị còn lại.
    let add = json!({"name": "_waf-verify.shop.test", "kind": "TXT", "value": "waf-verify-2", "replace": true});
    call(&app, "POST", "/v1/dns/records", Some(add)).await;
    let (_, got) = call(
        &app,
        "GET",
        "/resolve?name=_waf-verify.shop.test.&type=16",
        None,
    )
    .await;
    assert_eq!(got["Answer"].as_array().unwrap().len(), 1);
    assert_eq!(got["Answer"][0]["data"], "\"waf-verify-2\"");

    let (_, missing) = call(&app, "GET", "/resolve?name=nope.test&type=A", None).await;
    assert_eq!(missing["Status"], 3);
    assert!(missing.get("Answer").is_none());
}

#[tokio::test]
async fn a_cname_answer_names_its_target_with_a_dot() {
    let (app, _) = app().await;
    let add = json!({"name": "www.shop.test", "type": "CNAME", "value": "z1.edge.waf.local"});
    call(&app, "POST", "/v1/dns/records", Some(add)).await;
    let (_, got) = call(&app, "GET", "/resolve?name=www.shop.test&type=CNAME", None).await;
    assert_eq!(got["Answer"][0]["data"], "z1.edge.waf.local.");
}

#[tokio::test]
async fn bad_input_is_refused_and_nothing_is_deleted_without_a_name() {
    let (app, _) = app().await;
    for body in [
        json!({"name": "a.test", "type": "A", "value": "nope"}),
        json!({"name": "a.test"}),
    ] {
        let (status, got) = call(&app, "POST", "/v1/dns/records", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(got["error"]["code"], "invalid");
    }
    assert_eq!(
        call(&app, "DELETE", "/v1/dns/records", None).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&app, "GET", "/resolve?type=A", None).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&app, "DELETE", "/v1/dns/records/99", None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_servfail_fault_shows_in_doh_and_goes_away() {
    let (app, _) = app().await;
    let (status, made) = call(
        &app,
        "POST",
        "/v1/dns/faults",
        Some(json!({"name": "*.flaky.test", "action": "servfail"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(
        call(&app, "GET", "/resolve?name=a.flaky.test", None)
            .await
            .1["Status"],
        2
    );
    call(&app, "DELETE", "/v1/dns/faults", None).await;
    assert_eq!(
        call(&app, "GET", "/resolve?name=a.flaky.test", None)
            .await
            .1["Status"],
        3
    );
}

#[tokio::test]
async fn mail_is_read_by_recipient_and_reset_clears_everything() {
    let (app, storage) = app().await;
    assert_eq!(
        call(&app, "GET", "/v1/mail/messages/latest", None).await.0,
        StatusCode::NOT_FOUND
    );
    for (to, text) in [
        ("bob@mock.test", "one"),
        ("amy@mock.test", "two"),
        ("bob@mock.test", "three"),
    ] {
        let mail = NewMail {
            from: "app@mock.test".into(),
            to: vec![to.into()],
            text: text.into(),
            ..Default::default()
        };
        storage.save_mail(mail, 500).await.unwrap();
    }
    let (_, latest) = call(
        &app,
        "GET",
        "/v1/mail/messages/latest?to=BOB@mock.test",
        None,
    )
    .await;
    assert_eq!(latest["data"]["text"], "three");
    assert!(latest["data"]["receivedAt"].is_string());
    let (_, all) = call(&app, "GET", "/v1/mail/messages", None).await;
    assert_eq!(all["data"].as_array().unwrap().len(), 3);
    assert_eq!(
        call(&app, "DELETE", "/v1/reset", None).await.0,
        StatusCode::NO_CONTENT
    );
    let (_, all) = call(&app, "GET", "/v1/mail/messages", None).await;
    assert!(all["data"].as_array().unwrap().is_empty());
}

/// Gửi request thô; trả (status, content-type, body).
async fn raw(app: &Router, request: Request<Body>) -> (StatusCode, String, Vec<u8>) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let kind = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, kind, bytes.to_vec())
}

fn wire_query(name: &str) -> Vec<u8> {
    let mut message = Message::new(0, MessageType::Query, OpCode::Query);
    message.add_query(Query::query(Name::from_ascii(name).unwrap(), RecordType::A));
    message.to_vec().unwrap()
}

#[tokio::test]
async fn doh_answers_wire_queries_by_get_and_post_and_logs_them() {
    let (app, _) = app().await;
    let add = json!({"name": "app.test", "type": "A", "value": "127.0.0.9"});
    call(&app, "POST", "/v1/dns/records", Some(add)).await;

    let encoded = URL_SAFE_NO_PAD.encode(wire_query("app.test."));
    let get = Request::get(format!("/dns-query?dns={encoded}"));
    let post = Request::post("/dns-query").header("content-type", "application/dns-message");
    for request in [
        get.body(Body::empty()).unwrap(),
        post.body(Body::from(wire_query("app.test."))).unwrap(),
    ] {
        let (status, kind, bytes) = raw(&app, request).await;
        assert_eq!(
            (status, kind.as_str()),
            (StatusCode::OK, "application/dns-message")
        );
        let answer = Message::from_vec(&bytes).unwrap();
        assert_eq!(answer.answers[0].data.to_string(), "127.0.0.9");
        assert!(answer.metadata.authoritative);
    }

    for (request, want) in [
        (
            Request::get("/dns-query?dns=%21%21").body(Body::empty()),
            StatusCode::BAD_REQUEST,
        ),
        (
            Request::get("/dns-query").body(Body::empty()),
            StatusCode::BAD_REQUEST,
        ),
        (
            Request::post("/dns-query").body(Body::from(wire_query("app.test."))),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
    ] {
        assert_eq!(raw(&app, request.unwrap()).await.0, want);
    }

    call(&app, "GET", "/resolve?name=nope.test", None).await;
    let (_, log) = call(&app, "GET", "/v1/dns/queries", None).await;
    let rows = log["data"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "{log}");
    assert_eq!(rows[0]["proto"], "doh-json");
    assert_eq!(rows[0]["rcode"], "NXDOMAIN");
    assert_eq!(rows[0]["source"], "none");
    assert_eq!(rows[1]["proto"], "doh");
    assert_eq!(rows[1]["type"], "A");
    assert_eq!(rows[1]["answers"], 1);
    assert_eq!(rows[1]["source"], "mock");
    call(&app, "DELETE", "/v1/dns/queries", None).await;
    let (_, log) = call(&app, "GET", "/v1/dns/queries", None).await;
    assert!(log["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn with_a_token_the_admin_api_is_closed_but_queries_stay_open() {
    let storage = mockhub_storage::connect("sqlite::memory:").await.unwrap();
    let app = router(Engine::new(storage), 500, Some("s3cret"));
    for (method, uri) in [
        ("GET", "/v1/dns/records"),
        ("POST", "/v1/dns/faults"),
        ("GET", "/v1/dns/queries"),
        ("GET", "/v1/mail/messages"),
        ("GET", "/v1/mail/messages/latest"),
        ("DELETE", "/v1/reset"),
    ] {
        let (status, body) = call(&app, method, uri, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(body["error"]["code"], "unauthorized");
    }
    for (header, want) in [
        ("Bearer wrong", StatusCode::UNAUTHORIZED),
        ("Bearer s3cret", StatusCode::OK),
    ] {
        let request = Request::get("/v1/dns/records").header("authorization", header);
        assert_eq!(
            raw(&app, request.body(Body::empty()).unwrap()).await.0,
            want
        );
    }
    assert_eq!(call(&app, "GET", "/health", None).await.0, StatusCode::OK);
    assert_eq!(
        call(&app, "GET", "/resolve?name=a.test", None).await.0,
        StatusCode::OK
    );
    let encoded = URL_SAFE_NO_PAD.encode(wire_query("a.test."));
    let request = Request::get(format!("/dns-query?dns={encoded}"));
    assert_eq!(
        raw(&app, request.body(Body::empty()).unwrap()).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_web_guard_wants_basic_auth_and_refuses_cross_site_writes() {
    let app = Router::new()
        .route("/", axum::routing::get(|| async {}).post(|| async {}))
        .layer(axum::middleware::from_fn_with_state(
            Arc::<str>::from("s3cret"),
            auth::require_basic,
        ));
    // "any:s3cret"
    const GOOD: &str = "Basic YW55OnMzY3JldA==";
    let ask = |method: &str, auth: &str, origin: Option<&str>| {
        let mut request = Request::builder()
            .method(method)
            .uri("/")
            .header("host", "hub.test:8686")
            .header("authorization", auth);
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        request.body(Body::empty()).unwrap()
    };
    let status = async |request| raw(&app, request).await.0;
    assert_eq!(status(ask("GET", "", None)).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        status(ask("GET", "Bearer s3cret", None)).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(status(ask("GET", GOOD, None)).await, StatusCode::OK);
    assert_eq!(status(ask("POST", GOOD, None)).await, StatusCode::OK);
    let same = Some("http://hub.test:8686");
    assert_eq!(status(ask("POST", GOOD, same)).await, StatusCode::OK);
    for other in ["http://evil.test", "null"] {
        assert_eq!(
            status(ask("POST", GOOD, Some(other))).await,
            StatusCode::FORBIDDEN
        );
    }
}
