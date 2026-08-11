use axum::extract::{ConnectInfo, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use std::net::SocketAddr;

use crate::avatar;
use crate::db;
use crate::i18n;
use crate::identity;
use crate::models::{MeResponse, MessageKind, MessagesResponse, PostMessageRequest, SseEvent};
use crate::state::AppState;

const INDEX_HTML: &str = include_str!("../static/index.html");
const ICON_SVG: &str = include_str!("../assets/icon.svg");
const ICON_PNG_32: &[u8] = include_bytes!("../assets/icon-32.png");

pub async fn icon_svg() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], ICON_SVG)
}

pub async fn icon_png_32() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/png")], ICON_PNG_32)
}

pub struct AppError(anyhow::Error);

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!("request failed: {:?}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
    }
}

struct ClientIdentity {
    hostname: String,
    ip: String,
}

async fn client_identity(addr: &SocketAddr, state: &AppState) -> ClientIdentity {
    let ip = addr.ip();
    // Local access: present the server machine's real hostname and LAN IP
    // instead of "localhost (127.0.0.1)".
    if ip.is_loopback() {
        if let Some((hostname, ip_str)) =
            identity::local_identity(&state.server_ip, state.local_hostname.as_deref())
        {
            let _ = avatar::ensure_avatar(&state.data_dir.join("icons"), &hostname, &ip_str);
            return ClientIdentity {
                hostname,
                ip: ip_str,
            };
        }
    }
    // Reverse lookup can block on network I/O; never run it on the async executor.
    let hostname = tokio::task::spawn_blocking({
        let state = state.clone();
        move || state.resolve_hostname(ip)
    })
    .await
    .unwrap_or_else(|_| ip.to_string());
    let _ = avatar::ensure_avatar(&state.data_dir.join("icons"), &hostname, &ip.to_string());
    ClientIdentity {
        hostname,
        ip: ip.to_string(),
    }
}

pub async fn index(State(state): State<AppState>, headers: HeaderMap) -> Html<String> {
    let lang = i18n::detect(
        headers
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok()),
    );
    // Inject both languages so the client can switch without a reload.
    let all = serde_json::json!({
        "zh": i18n::strings(i18n::Lang::Zh),
        "en": i18n::strings(i18n::Lang::En),
    });
    let html = INDEX_HTML
        .replace("__SERVER_IP__", &state.server_ip)
        .replace("__LANG__", i18n::code(lang))
        .replace("__I18N_ALL_JSON__", &all.to_string());
    Html(html)
}

pub async fn me(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Json<MeResponse> {
    let id = client_identity(&addr, &state).await;
    Json(MeResponse {
        display: identity::display_name(&id.hostname, &id.ip),
        avatar_url: avatar::avatar_url(&id.hostname, &id.ip),
        hostname: id.hostname,
        ip: id.ip,
    })
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    before: Option<i64>,
    limit: Option<i64>,
}

pub async fn get_messages(
    State(state): State<AppState>,
    Query(q): Query<MessagesQuery>,
) -> Result<Json<MessagesResponse>, AppError> {
    let limit = q.limit.unwrap_or(10).clamp(1, 50);
    let (messages, has_more) = tokio::task::spawn_blocking({
        let state = state.clone();
        move || {
            let conn = state.db.lock().unwrap();
            db::fetch_messages(&conn, q.before, limit)
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))??;
    let dtos = messages.iter().map(|m| state.to_dto(m)).collect();
    Ok(Json(MessagesResponse {
        messages: dtos,
        has_more,
    }))
}

pub async fn post_message(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<PostMessageRequest>,
) -> Result<impl IntoResponse, AppError> {
    let content = req.content.trim().to_string();
    if content.is_empty() {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }
    let id = client_identity(&addr, &state).await;
    let message = tokio::task::spawn_blocking({
        let state = state.clone();
        let hostname = id.hostname.clone();
        let ip = id.ip.clone();
        move || {
            let conn = state.db.lock().unwrap();
            db::insert_message(
                &conn,
                &hostname,
                &ip,
                MessageKind::Text,
                Some(&content),
                None,
                None,
            )
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))??;
    let dto = state.to_dto(&message);
    let _ = state.tx.send(SseEvent::NewMessage { message: dto });
    tracing::info!("text message #{} from {} ({})", message.id, id.hostname, id.ip);
    Ok(StatusCode::CREATED.into_response())
}

enum StreamUploadError {
    TooLarge,
    Multipart(axum::extract::multipart::MultipartError),
    Io(std::io::Error),
}

/// Stream a multipart field to disk chunk by chunk, so upload memory usage
/// stays constant regardless of file size.
async fn stream_field_to_disk(
    mut field: axum::extract::multipart::Field<'_>,
    path: &std::path::Path,
) -> Result<u64, StreamUploadError> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(StreamUploadError::Io)?;
    let mut size: u64 = 0;
    loop {
        match field.chunk().await {
            Ok(Some(chunk)) => {
                file.write_all(&chunk).await.map_err(StreamUploadError::Io)?;
                size += chunk.len() as u64;
            }
            Ok(None) => break,
            Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                return Err(StreamUploadError::TooLarge);
            }
            Err(e) => return Err(StreamUploadError::Multipart(e)),
        }
    }
    file.shutdown().await.map_err(StreamUploadError::Io)?;
    Ok(size)
}

pub async fn upload(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    // Reject oversized uploads up front from Content-Length — otherwise the
    // client would have to stream a full gigabyte before getting the 413,
    // which over a LAN looks like the upload simply died.
    if let Some(len) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
    {
        if len > crate::MAX_BODY_SIZE as u64 {
            return Ok((
                StatusCode::PAYLOAD_TOO_LARGE,
                "file exceeds the 1GB upload limit",
            )
                .into_response());
        }
    }
    let id = client_identity(&addr, &state).await;
    loop {
        // Map extractor errors to their real status (413 for oversized
        // bodies) with an explicit message instead of a bare 500.
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                return Ok((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "file exceeds the 1GB upload limit",
                )
                    .into_response());
            }
            Err(e) => return Err(anyhow::anyhow!(e).into()),
        };
        if field.name() != Some("file") {
            continue;
        }
        let orig_name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unnamed".into());
        let content_type = field
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let kind = if content_type.starts_with("image/") {
            MessageKind::Image
        } else {
            MessageKind::File
        };
        let safe_name: String = orig_name
            .chars()
            .map(|c| if c == '/' || c == '\\' || c == '\0' { '_' } else { c })
            .collect();
        let stored_name = format!("{}_{}", uuid::Uuid::new_v4(), safe_name);
        // Write to a hidden temp file first; only a completed upload gets its
        // final name, and any failure removes the partial file.
        let tmp_path = state.data_dir.join(format!(".{stored_name}.part"));
        let size = match stream_field_to_disk(field, &tmp_path).await {
            Ok(s) => s,
            Err(StreamUploadError::TooLarge) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Ok((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "file exceeds the 1GB upload limit",
                )
                    .into_response());
            }
            Err(StreamUploadError::Multipart(e)) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(anyhow::anyhow!(e).into());
            }
            Err(StreamUploadError::Io(e)) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(anyhow::anyhow!(e).into());
            }
        };
        if size == 0 {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Ok((StatusCode::BAD_REQUEST, "empty file").into_response());
        }
        tokio::fs::rename(&tmp_path, state.data_dir.join(&stored_name))
            .await
            .map_err(anyhow::Error::from)?;
        let message = tokio::task::spawn_blocking({
            let state = state.clone();
            let hostname = id.hostname.clone();
            let ip = id.ip.clone();
            let stored = stored_name.clone();
            let orig = orig_name.clone();
            move || {
                let conn = state.db.lock().unwrap();
                db::insert_message(
                    &conn,
                    &hostname,
                    &ip,
                    kind,
                    None,
                    Some(&stored),
                    Some(&orig),
                )
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))??;
        let dto = state.to_dto(&message);
        let _ = state.tx.send(SseEvent::NewMessage { message: dto });
        tracing::info!(
            "upload #{} from {} ({}): {} ({} bytes)",
            message.id,
            id.hostname,
            id.ip,
            orig_name,
            size
        );
        return Ok(StatusCode::CREATED.into_response());
    }
    Ok((StatusCode::BAD_REQUEST, "no file field").into_response())
}

pub async fn recall_message(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let ident = client_identity(&addr, &state).await;
    let result = tokio::task::spawn_blocking({
        let state = state.clone();
        let hostname = ident.hostname.clone();
        let ip = ident.ip.clone();
        move || {
            let conn = state.db.lock().unwrap();
            db::recall_message(&conn, id, &hostname, &ip)
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;
    match result {
        Ok(true) => {
            let _ = state.tx.send(SseEvent::Recalled { id });
            tracing::info!("message #{} recalled by {} ({})", id, ident.hostname, ident.ip);
            Ok(StatusCode::OK.into_response())
        }
        Ok(false) => {
            tracing::warn!(
                "recall of message #{} denied for {} ({})",
                id,
                ident.hostname,
                ident.ip
            );
            Ok((StatusCode::FORBIDDEN, "not the message owner").into_response())
        }
        Err(e) => Ok((StatusCode::NOT_FOUND, e.to_string()).into_response()),
    }
}
