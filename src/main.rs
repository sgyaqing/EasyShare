use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use rusqlite::Connection;
use tokio::sync::broadcast;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;

use easyshare::models::SseEvent;
use easyshare::state::AppState;

#[derive(Parser)]
#[command(name = "easyshare", version, about = "A single-executable file sharing server")]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value_t = 8972)]
    port: u16,
}

/// Directory next to the executable, falling back to the current directory.
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn detect_lan_ip() -> String {
    local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let base = exe_dir();

    // Everything runtime-generated lives under easyshare-files/, so the
    // whole deployment is just the executable plus that one directory.
    let data_dir = base.join("easyshare-files");
    let db_dir = data_dir.join("db");
    let logs_dir = data_dir.join("logs");
    fs::create_dir_all(data_dir.join("icons")).context("failed to create easyshare-files directory")?;
    fs::create_dir_all(&db_dir).context("failed to create easyshare-files/db directory")?;
    fs::create_dir_all(&logs_dir).context("failed to create easyshare-files/logs directory")?;

    // Logs: stdout + daily rolling file under easyshare-files/logs/.
    let file_appender = tracing_appender::rolling::daily(&logs_dir, "easyshare.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);
    let stdout_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stdout);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false);
    tracing_subscriber::registry()
        .with(stdout_layer.with_filter(LevelFilter::INFO))
        .with(file_layer.with_filter(LevelFilter::INFO))
        .init();

    let db_path = db_dir.join("easyshare.db");
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open database {}", db_path.display()))?;
    easyshare::db::init(&conn)?;

    // The advertised IP (page title, startup banner, local identity) can be
    // overridden — mainly for containers, where auto-detection only sees the
    // container's internal address.
    let server_ip = match std::env::var("EASY_SHARE_HOST_IP") {
        Ok(s) if !s.trim().is_empty() => {
            let s = s.trim().to_string();
            if s.parse::<std::net::IpAddr>().is_ok() {
                s
            } else {
                tracing::warn!("ignoring invalid EASY_SHARE_HOST_IP value: {s}");
                detect_lan_ip()
            }
        }
        _ => detect_lan_ip(),
    };

    // Hostname override for the local identity — containers see the VM's
    // hostname ("docker-desktop"), not the host machine's.
    let local_hostname = std::env::var("EASY_SHARE_HOSTNAME")
        .ok()
        .map(|s| s.trim().trim_end_matches('.').to_string())
        .filter(|s| !s.is_empty());

    let (tx, _rx) = broadcast::channel::<SseEvent>(256);
    let state = AppState::new(
        conn,
        tx,
        data_dir.clone(),
        server_ip.clone(),
        local_hostname,
    );

    let app = easyshare::build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], cli.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    let url = format!("http://{server_ip}:{}", cli.port);
    println!("Server listening on {url}");
    println!("Copy the URL above into your browser to get started.");
    tracing::info!("server listening on {url}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
