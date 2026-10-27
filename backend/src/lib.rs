// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Sorobo – lokale Buchhaltungs- und Rechnungssoftware.
//!
//! The library contains the complete application: database, bookkeeping, PDF output and the
//! HTTP API including the embedded frontend. It is used by two programs:
//!
//! * `sorobo-server` – headless server for the browser or the Vite dev server,
//! * `sorobo-desktop` – native window (WebView2 / WebKit) around the same server.

mod api;
mod chart;
mod db;
mod error;
mod guard;
mod models;
mod pdf;
mod services;
mod state;
mod util;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use tower_http::compression::CompressionLayer;

use crate::state::{AppState, Inner};

/// Environment variable `SOROBO_<name>`. Until version 0.10 the program was called "Sqonto";
/// `SQONTO_<name>` is still read, so existing scripts and shortcuts keep working.
pub fn env_var(name: &str) -> Option<String> {
    std::env::var(format!("SOROBO_{name}")).or_else(|_| std::env::var(format!("SQONTO_{name}"))).ok()
}

/// Data directory: `SOROBO_DATA_DIR`, otherwise next to the executable for release builds
/// (portable installation, independent of where it is started from) and the working directory
/// for debug builds.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = env_var("DATA_DIR") {
        return PathBuf::from(dir);
    }
    if !cfg!(debug_assertions)
        && let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()))
    {
        return dir.join("data");
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join("data")
}

/// Preferred port: `SOROBO_PORT` or 8787.
pub fn default_port() -> u16 {
    env_var("PORT").and_then(|p| p.parse().ok()).unwrap_or(8787)
}

/// A bound, fully initialised server that only needs to be run.
pub struct Server {
    listener: tokio::net::TcpListener,
    app: axum::Router,
    pub port: u16,
    pub url: String,
    pub data_dir: PathBuf,
}

impl Server {
    pub async fn run(self) -> anyhow::Result<()> {
        axum::serve(self.listener, self.app).await?;
        Ok(())
    }
}

pub enum Launch {
    Started(Server),
    /// Sorobo already runs with the same data (e.g. after a second double-click).
    AlreadyRunning { port: u16, url: String },
}

/// Binds the local port and initialises database, fonts and API. A port taken by anything
/// other than a Sorobo instance with the same data is skipped.
pub async fn launch(port: u16, dir: &Path) -> anyhow::Result<Launch> {
    std::fs::create_dir_all(dir.join("files"))?;
    // Started by an update: the previous process is still shutting down and keeps its port a moment.
    // A version from before the rename sets `SQONTO_RESTARTING`.
    let restarting = env_var("RESTARTING").is_some();
    let mut bound = None;
    for p in port..port.saturating_add(20) {
        match bind(p, restarting && p == port).await {
            Ok(listener) => {
                bound = Some((listener, p));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if running_instance(p).is_some_and(|d| same_dir(Path::new(&d), dir)) {
                    return Ok(Launch::AlreadyRunning { port: p, url: format!("http://localhost:{p}") });
                }
                tracing::warn!("Port {p} ist belegt, versuche {} …", p + 1);
            }
            Err(e) => return Err(e.into()),
        }
    }
    let Some((listener, port)) = bound else {
        anyhow::bail!("Kein freier Port zwischen {port} und {} gefunden.", port.saturating_add(19));
    };
    migrate_legacy_files(dir)?;
    let conn = db::open(&dir.join("sorobo.db"))?;
    services::settings::ensure_seeded(&conn)?;
    services::posting::ensure_current_afa(&conn)?;
    tracing::info!("Lade Schriftarten …");
    let pdf = pdf::PdfEngine::new();
    tracing::info!("{} Schriftfamilien verfügbar", pdf.families().len());
    let state = AppState(Arc::new(Inner { db: Mutex::new(conn), data_dir: dir.to_path_buf(), pdf }));
    spawn_maintenance(state.clone());
    spawn_update_check(state.clone());
    let app = api::router(state).layer(CompressionLayer::new()).layer(axum::middleware::from_fn(guard::local_only));
    Ok(Launch::Started(Server { listener, app, port, url: format!("http://localhost:{port}"), data_dir: dir.to_path_buf() }))
}

/// Binds the local port; `wait`: retry for up to 30 s while it is still in use.
async fn bind(port: u16, wait: bool) -> std::io::Result<tokio::net::TcpListener> {
    let mut attempts = if wait { 120 } else { 0 };
    loop {
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && attempts > 0 => {
                attempts -= 1;
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            result => return result,
        }
    }
}

/// Looks for a new version at the start and then daily, unless switched off in the settings.
fn spawn_update_check(state: AppState) {
    tokio::spawn(async move {
        let mut daily = tokio::time::interval(Duration::from_secs(24 * 3600));
        loop {
            daily.tick().await;
            let st = state.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if st.read(services::settings::load).is_ok_and(|s| s.updates.auto_check) {
                    services::update::check();
                }
            })
            .await;
        }
    });
}

/// Hourly upkeep while the program runs, starting right away: the daily automatic backup, due
/// Abo-Rechnungen and the depreciation of a new year – also when the program keeps running over
/// New Year. Each only does work when it is due.
fn spawn_maintenance(state: AppState) {
    tokio::spawn(async move {
        let mut hourly = tokio::time::interval(Duration::from_secs(3600));
        loop {
            hourly.tick().await;
            let st = state.clone();
            // Backups read many files – keep them off the async workers.
            let created = tokio::task::spawn_blocking(move || {
                if let Err(e) = st.write(services::posting::ensure_current_afa) {
                    tracing::warn!("Abschreibungen des neuen Jahres: {}", e.message);
                }
                let created = match st.write(|c| services::recurring::create_due(c, util::today())) {
                    Ok(created) => {
                        if !created.is_empty() {
                            tracing::info!("{} Rechnungen aus Abo-Rechnungen erstellt", created.len());
                        }
                        created
                    }
                    Err(e) => {
                        tracing::warn!("Abo-Rechnungen: {}", e.message);
                        Vec::new()
                    }
                };
                services::update::cleanup(&st.updates_dir());
                match api::auto_backup(&st, false) {
                    Ok(Some(path)) => tracing::info!("Automatische Sicherung: {}", path.display()),
                    Ok(None) => {}
                    Err(e) => tracing::warn!("Automatische Sicherung fehlgeschlagen: {}", e.message),
                }
                created
            })
            .await
            .unwrap_or_default();
            // Finalized Abo-Rechnungen are archived and sent (rendering and SMTP are async).
            api::deliver_recurring(&state, created).await;
        }
    });
}

/// Earlier names of the program, newest first: "Sqonto" until version 0.10, "Kontor" before.
const LEGACY_NAMES: [&str; 2] = ["sqonto", "kontor"];

/// Takes over the database of an earlier name unchanged (together with its WAL files, which
/// SQLite finds by the database's name) and removes the old log.
fn migrate_legacy_files(dir: &Path) -> anyhow::Result<()> {
    if dir.join("sorobo.db").exists() {
        return Ok(());
    }
    let Some(old) = LEGACY_NAMES.into_iter().find(|n| dir.join(format!("{n}.db")).exists()) else {
        return Ok(());
    };
    for suffix in ["", "-wal", "-shm"] {
        let from = dir.join(format!("{old}.db{suffix}"));
        if from.exists() {
            std::fs::rename(&from, dir.join(format!("sorobo.db{suffix}")))?;
        }
    }
    for name in LEGACY_NAMES {
        let _ = std::fs::remove_file(dir.join(format!("{name}.log")));
    }
    tracing::info!("Datenbank {old}.db als sorobo.db übernommen");
    Ok(())
}

/// Minimal HTTP/1.0 request to a local instance; returns the parsed JSON body.
fn local_request(port: u16, method: &str, path: &str) -> Option<serde_json::Value> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let request = format!("{method} {path} HTTP/1.0\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    serde_json::from_str(response.split_once("\r\n\r\n")?.1).ok()
}

/// Data directory of a Sorobo instance listening on `port`, if there is one.
fn running_instance(port: u16) -> Option<String> {
    local_request(port, "GET", "/api/system/status")?.get("data_dir")?.as_str().map(str::to_string)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

static FOCUS_HANDLER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Registers what happens when another start of the program asks this instance to come to the
/// front (the desktop app raises its window).
pub fn set_focus_handler(f: impl Fn() + Send + Sync + 'static) {
    let _ = FOCUS_HANDLER.set(Box::new(f));
}

static QUIT_HANDLER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Registers how the program ends itself after an update (the desktop app closes its window).
pub fn set_quit_handler(f: impl Fn() + Send + Sync + 'static) {
    let _ = QUIT_HANDLER.set(Box::new(f));
}

/// Ends the program for an update; without a handler (headless server) the process exits.
pub(crate) fn quit() {
    match QUIT_HANDLER.get() {
        Some(f) => f(),
        None => std::process::exit(0),
    }
}

/// Called by `POST /api/system/focus`; false if this instance has no window.
pub(crate) fn focus_window() -> bool {
    FOCUS_HANDLER.get().map(|f| f()).is_some()
}

/// Asks the instance on `port` to raise its window; false if it has none (headless server).
pub fn request_focus(port: u16) -> bool {
    local_request(port, "POST", "/api/system/focus").and_then(|v| v.get("focused")?.as_bool()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sorobo-legacy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut n: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        n.sort();
        n
    }

    #[test]
    fn database_of_an_earlier_name_is_taken_over() {
        let dir = temp_dir();
        for (file, body) in [("sqonto.db", "db"), ("sqonto.db-wal", "wal"), ("sqonto.db-shm", "shm"), ("sqonto.log", "log"), ("kontor.db", "älter")] {
            std::fs::write(dir.join(file), body).unwrap();
        }
        migrate_legacy_files(&dir).unwrap();
        // The newest earlier name wins; the older database is left alone.
        assert_eq!(names(&dir), ["kontor.db", "sorobo.db", "sorobo.db-shm", "sorobo.db-wal"]);
        assert_eq!(std::fs::read_to_string(dir.join("sorobo.db")).unwrap(), "db");
        assert_eq!(std::fs::read_to_string(dir.join("sorobo.db-wal")).unwrap(), "wal");
        // Runs again on every start: an existing sorobo.db is never touched.
        std::fs::write(dir.join("sqonto.db"), "neu angelegt").unwrap();
        migrate_legacy_files(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("sorobo.db")).unwrap(), "db");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn database_from_kontor_is_still_taken_over() {
        let dir = temp_dir();
        std::fs::write(dir.join("kontor.db"), "db").unwrap();
        std::fs::write(dir.join("kontor.log"), "log").unwrap();
        migrate_legacy_files(&dir).unwrap();
        assert_eq!(names(&dir), ["sorobo.db"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_happens_without_an_earlier_database() {
        let dir = temp_dir();
        migrate_legacy_files(&dir).unwrap();
        assert!(names(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
