use std::net::SocketAddr;

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use rusqlite::Connection;
use tempfile::TempDir;
use tokio::sync::broadcast;
use tower::ServiceExt;

use easyshare::models::{MessagesResponse, SseEvent};
use easyshare::state::AppState;

struct TestApp {
    router: axum::Router,
    state: AppState,
    _tmp: TempDir,
}

fn make_app() -> TestApp {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("easyshare-files");
    std::fs::create_dir_all(data_dir.join("icons")).unwrap();
    let conn = Connection::open_in_memory().unwrap();
    easyshare::db::init(&conn).unwrap();
    let (tx, _rx) = broadcast::channel::<SseEvent>(256);
    let state = AppState::new(conn, tx, data_dir, "10.1.2.3".into(), None);
    let router = easyshare::build_router(state.clone());
    TestApp {
        router,
        state,
        _tmp: tmp,
    }
}

fn addr_a() -> SocketAddr {
    "127.0.0.1:50001".parse().unwrap()
}

// Non-loopback so it does NOT map to the local-machine identity.
fn addr_b() -> SocketAddr {
    "192.168.99.2:50002".parse().unwrap()
}

fn request(method: &str, uri: &str, addr: SocketAddr) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    req
}

fn json_request(uri: &str, addr: SocketAddr, json: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(json.to_string()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    req
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn post_text(app: &TestApp, addr: SocketAddr, text: &str) -> StatusCode {
    let body = serde_json::json!({"content": text}).to_string();
    app.router
        .clone()
        .oneshot(json_request("/api/messages", addr, &body))
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn index_injects_ip_and_english_strings() {
    let app = make_app();
    let mut req = request("GET", "/", addr_a());
    req.headers_mut()
        .insert("accept-language", "en-US,en;q=0.9".parse().unwrap());
    let resp = app.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("<title>EasyShare - 10.1.2.3</title>"));
    assert!(html.contains("window.LANG = \"en\""));
    // Both languages are injected so the client can switch without reload.
    assert!(html.contains("\"send\":\"Send\""));
    assert!(html.contains("\"send\":\"发送\""));
    assert!(!html.contains("__SERVER_IP__"));
    assert!(!html.contains("__LANG__"));
    assert!(!html.contains("__I18N_ALL_JSON__"));
}

#[tokio::test]
async fn index_injects_chinese_strings() {
    let app = make_app();
    let mut req = request("GET", "/", addr_a());
    req.headers_mut()
        .insert("accept-language", "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());
    let resp = app.router.clone().oneshot(req).await.unwrap();
    let html = body_string(resp).await;
    assert!(html.contains("window.LANG = \"zh\""));
    assert!(html.contains("\"send\":\"发送\""));
    assert!(html.contains("撤回"));
}

#[tokio::test]
async fn me_returns_identity_and_caches_avatar() {
    let app = make_app();
    let resp = app
        .router
        .clone()
        .oneshot(request("GET", "/api/me", addr_a()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    // Loopback clients get the server machine's LAN identity.
    assert_eq!(v["ip"], "10.1.2.3");
    let hostname = v["hostname"].as_str().unwrap();
    assert!(!hostname.is_empty());
    let display = v["display"].as_str().unwrap();
    assert!(display.ends_with(" (10.1.2.3)"), "display: {display}");
    assert!(display.starts_with(hostname), "display: {display}");
    let avatar_url = v["avatar_url"].as_str().unwrap();
    assert!(avatar_url.starts_with("/files/icons/"));

    // Avatar SVG written to icons dir; second call reuses the same file.
    let icons_dir = app.state.data_dir.join("icons");
    let file_name = avatar_url.trim_start_matches("/files/icons/");
    let path = icons_dir.join(file_name);
    assert!(path.exists(), "avatar file missing: {}", path.display());
    let first = std::fs::read_to_string(&path).unwrap();
    assert!(first.contains("<svg"));
    let resp2 = app
        .router
        .clone()
        .oneshot(request("GET", "/api/me", addr_a()))
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
}

#[tokio::test]
async fn text_message_roundtrip_and_pagination() {
    let app = make_app();
    for i in 0..25 {
        let status = post_text(&app, addr_a(), &format!("message {i}")).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    // Page 1: latest 10.
    let resp = app
        .router
        .clone()
        .oneshot(request("GET", "/api/messages?limit=10", addr_a()))
        .await
        .unwrap();
    let page1: MessagesResponse =
        serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(page1.messages.len(), 10);
    assert!(page1.has_more);
    assert_eq!(page1.messages[9].content.as_deref(), Some("message 24"));

    // Page 2.
    let uri = format!("/api/messages?limit=10&before={}", page1.messages[0].id);
    let resp = app
        .router
        .clone()
        .oneshot(request("GET", &uri, addr_a()))
        .await
        .unwrap();
    let page2: MessagesResponse =
        serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(page2.messages.len(), 10);
    assert!(page2.has_more);
    assert_eq!(page2.messages[9].content.as_deref(), Some("message 14"));

    // Page 3: 5 remaining, no more.
    let uri = format!("/api/messages?limit=10&before={}", page2.messages[0].id);
    let resp = app
        .router
        .clone()
        .oneshot(request("GET", &uri, addr_a()))
        .await
        .unwrap();
    let page3: MessagesResponse =
        serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(page3.messages.len(), 5);
    assert!(!page3.has_more);
    assert_eq!(page3.messages[0].content.as_deref(), Some("message 0"));

    // Page 4: empty.
    let uri = format!("/api/messages?limit=10&before={}", page3.messages[0].id);
    let resp = app
        .router
        .clone()
        .oneshot(request("GET", &uri, addr_a()))
        .await
        .unwrap();
    let page4: MessagesResponse =
        serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(page4.messages.is_empty());
    assert!(!page4.has_more);
}

fn multipart_body(field: &str, filename: &str, content_type: &str, data: &[u8]) -> Body {
    let boundary = "TESTBOUNDARY";
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    buf.extend_from_slice(data);
    buf.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Body::from(buf)
}

fn upload_request(addr: SocketAddr, filename: &str, content_type: &str, data: &[u8]) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/upload")
        .header(
            "content-type",
            "multipart/form-data; boundary=TESTBOUNDARY",
        )
        .body(multipart_body("file", filename, content_type, data))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    req
}

async fn latest_message(app: &TestApp) -> easyshare::models::MessageDto {
    let resp = app
        .router
        .clone()
        .oneshot(request("GET", "/api/messages?limit=1", addr_a()))
        .await
        .unwrap();
    let page: MessagesResponse = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(page.messages.len(), 1);
    page.messages.into_iter().next().unwrap()
}

#[tokio::test]
async fn upload_image_roundtrip() {
    let app = make_app();
    let png_bytes: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 1, 2, 3, 4, 5];
    let resp = app
        .router
        .clone()
        .oneshot(upload_request(addr_a(), "pic.png", "image/png", &png_bytes))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let msg = latest_message(&app).await;
    assert_eq!(msg.kind, easyshare::models::MessageKind::Image);
    assert_eq!(msg.orig_name.as_deref(), Some("pic.png"));
    assert_eq!(msg.size, Some(png_bytes.len() as u64));
    let file_url = msg.file_url.clone().unwrap();

    // File exists on disk with identical bytes.
    let stored = file_url.trim_start_matches("/files/");
    let on_disk = std::fs::read(app.state.data_dir.join(stored)).unwrap();
    assert_eq!(on_disk, png_bytes);

    // Served over HTTP byte-identical.
    let resp = app
        .router
        .clone()
        .oneshot(request("GET", &file_url, addr_b()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.as_ref(), png_bytes.as_slice());
}

#[tokio::test]
async fn upload_binary_is_file_kind() {
    let app = make_app();
    let data: Vec<u8> = vec![0, 1, 2, 3, 255, 254];
    let resp = app
        .router
        .clone()
        .oneshot(upload_request(
            addr_a(),
            "archive.bin",
            "application/octet-stream",
            &data,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let msg = latest_message(&app).await;
    assert_eq!(msg.kind, easyshare::models::MessageKind::File);
    assert_eq!(msg.orig_name.as_deref(), Some("archive.bin"));
}

#[tokio::test]
async fn upload_larger_than_axum_default_limit() {
    // Axum's default body limit is 2 MiB; files must be allowed to exceed it.
    let app = make_app();
    let data: Vec<u8> = vec![7u8; 3 * 1024 * 1024]; // 3 MiB
    let resp = app
        .router
        .clone()
        .oneshot(upload_request(
            addr_a(),
            "big.bin",
            "application/octet-stream",
            &data,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let msg = latest_message(&app).await;
    assert_eq!(msg.size, Some(data.len() as u64));
    let file_url = msg.file_url.clone().unwrap();
    let on_disk = std::fs::read(
        app.state.data_dir.join(file_url.trim_start_matches("/files/")),
    )
    .unwrap();
    assert_eq!(on_disk.len(), data.len());
}

#[tokio::test]
async fn recall_owner_ok_other_forbidden() {
    let app = make_app();
    assert_eq!(post_text(&app, addr_a(), "secret").await, StatusCode::CREATED);
    let msg = latest_message(&app).await;

    // Different client cannot recall.
    let uri = format!("/api/messages/{}/recall", msg.id);
    let resp = app
        .router
        .clone()
        .oneshot(request("POST", &uri, addr_b()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Owner can.
    let resp = app
        .router
        .clone()
        .oneshot(request("POST", &uri, addr_a()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let after = latest_message(&app).await;
    assert!(after.recalled);
}

#[tokio::test]
async fn sse_broadcasts_new_message_and_recall() {
    let app = make_app();
    let mut rx = app.state.tx.subscribe();

    assert_eq!(post_text(&app, addr_a(), "live").await, StatusCode::CREATED);
    let ev = rx.recv().await.unwrap();
    let msg_id = match ev {
        SseEvent::NewMessage { message } => {
            assert_eq!(message.content.as_deref(), Some("live"));
            message.id
        }
        other => panic!("expected NewMessage, got {other:?}"),
    };

    let uri = format!("/api/messages/{msg_id}/recall");
    let resp = app
        .router
        .clone()
        .oneshot(request("POST", &uri, addr_a()))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ev = rx.recv().await.unwrap();
    match ev {
        SseEvent::Recalled { id } => assert_eq!(id, msg_id),
        other => panic!("expected Recalled, got {other:?}"),
    }
}

#[tokio::test]
async fn upload_rejected_early_by_content_length() {
    // A declared 2 GiB body must be refused before any bytes are read.
    let app = make_app();
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/upload")
        .header("content-type", "multipart/form-data; boundary=TESTBOUNDARY")
        .header("content-length", (2u64 * 1024 * 1024 * 1024).to_string())
        .body(Body::from("tiny"))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(addr_a()));
    let resp = app.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn empty_text_rejected() {
    let app = make_app();
    assert_eq!(post_text(&app, addr_a(), "   ").await, StatusCode::BAD_REQUEST);
}
