use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tokio::sync::broadcast;

use crate::avatar;
use crate::models::{Message, MessageDto, SseEvent};

const IDENTITY_CACHE_TTL: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub tx: broadcast::Sender<SseEvent>,
    pub data_dir: PathBuf,
    pub server_ip: String,
    pub local_hostname: Option<String>,
    identity_cache: Arc<Mutex<HashMap<IpAddr, (String, Instant)>>>,
}

impl AppState {
    pub fn new(
        db: Connection,
        tx: broadcast::Sender<SseEvent>,
        data_dir: PathBuf,
        server_ip: String,
        local_hostname: Option<String>,
    ) -> Self {
        AppState {
            db: Arc::new(Mutex::new(db)),
            tx,
            data_dir,
            server_ip,
            local_hostname,
            identity_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Cached hostname for a client IP. Reverse lookups (mDNS/DNS) can take
    /// up to ~1s for unresponsive devices, so results — positive or negative
    /// — are cached for 10 minutes.
    pub fn resolve_hostname(&self, ip: IpAddr) -> String {
        if let Some((name, at)) = self.identity_cache.lock().unwrap().get(&ip) {
            if at.elapsed() < IDENTITY_CACHE_TTL {
                return name.clone();
            }
        }
        let name = crate::identity::resolve_hostname(ip);
        self.identity_cache
            .lock()
            .unwrap()
            .insert(ip, (name.clone(), Instant::now()));
        name
    }

    pub fn to_dto(&self, m: &Message) -> MessageDto {
        let size = m
            .filename
            .as_ref()
            .and_then(|f| std::fs::metadata(self.data_dir.join(f)).ok())
            .map(|md| md.len());
        MessageDto {
            id: m.id,
            sender: format!("{} ({})", m.hostname, m.ip),
            hostname: m.hostname.clone(),
            ip: m.ip.clone(),
            avatar_url: avatar::avatar_url(&m.hostname, &m.ip),
            kind: m.kind,
            content: m.content.clone(),
            file_url: m.filename.as_ref().map(|f| format!("/files/{f}")),
            orig_name: m.orig_name.clone(),
            size,
            created_at: m.created_at,
            recalled: m.recalled,
        }
    }
}
