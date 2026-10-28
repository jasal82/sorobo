// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

use std::io::{Read, Write};

use axum::Json;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderValue, header};
use axum::response::Response;
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Pages, file_response, render_svgs};
use crate::db::{self, v};
use crate::error::{ApiResult, AppError};
use crate::models::*;
use crate::pdf::{PdfEngine, data};
use crate::services::{demo, finance, masterdata, posting, settings as cfg};
use crate::state::AppState;
use crate::util::{now_str, today, ymd};

#[derive(Serialize, TS)]
#[ts(export)]
pub struct SystemStatus {
    pub onboarded: bool,
    pub has_data: bool,
    pub version: String,
    pub data_dir: String,
    pub chart: String,
    /// The chart can no longer be switched (accounts are in use).
    pub chart_locked: bool,
    pub bookkeeping: String,
    pub small_business: bool,
    pub company_name: String,
    pub has_cash: bool,
    pub default_cash_id: Option<i64>,
    pub default_bank_id: Option<i64>,
}

/// Raises the desktop window (sent by a second start of the program).
pub async fn focus() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "focused": crate::focus_window() }))
}

pub async fn status(State(st): State<AppState>) -> ApiResult<SystemStatus> {
    st.read(|c| {
        let s = cfg::load(c)?;
        let has_data = db::scalar_i64(c, "SELECT (SELECT COUNT(*) FROM documents) + (SELECT COUNT(*) FROM receipts) + (SELECT COUNT(*) FROM transactions)", &[])? > 0;
        let cash: Option<i64> = c.query_row("SELECT id FROM payment_accounts WHERE kind = 'cash' AND archived = 0 ORDER BY id LIMIT 1", [], |r| r.get(0)).ok();
        let bank: Option<i64> = c.query_row("SELECT id FROM payment_accounts WHERE kind != 'cash' AND archived = 0 ORDER BY id LIMIT 1", [], |r| r.get(0)).ok();
        Ok(SystemStatus {
            onboarded: s.onboarded,
            has_data,
            version: env!("CARGO_PKG_VERSION").into(),
            data_dir: st.data_dir.display().to_string(),
            chart: s.tax.chart.clone(),
            chart_locked: cfg::chart_in_use(c)?,
            bookkeeping: s.tax.bookkeeping.clone(),
            small_business: s.tax.small_business,
            company_name: s.company.name.clone(),
            has_cash: cash.is_some(),
            default_cash_id: cash,
            default_bank_id: bank,
        })
    })
    .map(Json)
}

pub async fn get_settings(State(st): State<AppState>) -> ApiResult<Settings> {
    st.read(cfg::load).map(|s| Json(without_password(s)))
}

/// The SMTP password stays in the backend.
fn without_password(mut s: Settings) -> Settings {
    s.email.has_password = !s.email.password.is_empty();
    s.email.password.clear();
    s
}

/// Drops references to files or layouts that no longer exist (e.g. after a reset or restore).
fn sanitize_refs(c: &rusqlite::Connection, s: &mut Settings) -> anyhow::Result<()> {
    if let Some(id) = s.company.logo_file_id
        && db::find::<FileMeta>(c, id)?.is_none()
    {
        s.company.logo_file_id = None;
    }
    if let Some(id) = s.docs.default_layout_id
        && db::find::<Layout>(c, id)?.is_none()
    {
        s.docs.default_layout_id = None;
    }
    Ok(())
}

pub async fn put_settings(State(st): State<AppState>, Json(mut new): Json<Settings>) -> ApiResult<Settings> {
    st.write(|c| {
        let old = cfg::load(c)?;
        if new.tax.chart != old.tax.chart {
            cfg::change_chart(c, &new.tax.chart)?;
        }
        new.onboarded = old.onboarded || new.onboarded;
        // The locked period only changes through `lock_period`.
        new.tax.locked_until = old.tax.locked_until;
        if new.email.password.is_empty() {
            new.email.password = old.email.password.clone();
        }
        if new.email.enabled {
            crate::services::email::check_settings(&new.email)?;
        }
        sanitize_refs(c, &mut new)?;
        if new.dunning.levels.is_empty() {
            new.dunning.levels = old.dunning.levels.clone();
        }
        let folder = new.backup.folder.trim();
        if !folder.is_empty() && (!std::path::Path::new(folder).is_absolute() || std::fs::create_dir_all(folder).is_err()) {
            crate::bail_user!("Der Sicherungsordner „{folder}“ ist nicht verfügbar. Bitte einen vollständigen Pfad zu einem beschreibbaren Ordner angeben.");
        }
        let rebuild = new.tax.bookkeeping != old.tax.bookkeeping || new.tax.small_business != old.tax.small_business;
        if rebuild && old.tax.locked_until.is_some() {
            crate::bail_user!("Buchführungsart und Kleinunternehmerstatus können nach einer Festschreibung nicht mehr geändert werden.");
        }
        cfg::save(c, &new)?;
        if rebuild {
            posting::rebuild_all(c)?;
        }
        Ok(without_password(new))
    })
    .map(Json)
}

/// Sends a test message to the sender address with the saved settings.
pub async fn email_test(State(st): State<AppState>) -> ApiResult<super::MessageResponse> {
    let s = st.read(cfg::load)?;
    let to = s.email.from_address.trim().to_string();
    let msg = crate::services::email::Outgoing {
        to: vec![to.clone()],
        cc: vec![],
        subject: "Sorobo: Test-E-Mail".into(),
        body: "Diese Nachricht bestätigt, dass Sorobo über Ihren E-Mail-Server senden kann.".into(),
        attachments: vec![],
    };
    tokio::task::spawn_blocking(move || crate::services::email::send(&s.email, &msg)).await.map_err(|e| AppError::from(anyhow::anyhow!(e)))??;
    Ok(Json(super::MessageResponse { message: format!("Test-E-Mail an {to} gesendet") }))
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct GettingStartedInput {
    pub hidden: bool,
}

/// Shows or hides the "Erste Schritte" checklist on the dashboard without touching other settings.
pub async fn put_getting_started(State(st): State<AppState>, Json(input): Json<GettingStartedInput>) -> ApiResult<Settings> {
    st.write(|c| {
        let mut s = cfg::load(c)?;
        s.hide_getting_started = input.hidden;
        cfg::save(c, &s)?;
        Ok(without_password(s))
    })
    .map(Json)
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct LockRequest {
    pub until: NaiveDate,
}

/// Festschreibung (GoBD): extends the locked period; it can never be shortened or lifted.
pub async fn lock_period(State(st): State<AppState>, Json(req): Json<LockRequest>) -> ApiResult<Settings> {
    st.write(|c| cfg::lock_until(c, req.until)).map(|s| Json(without_password(s)))
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct OnboardingInput {
    pub settings: Settings,
    pub bank_name: String,
    pub bank_iban: String,
    #[ts(type = "number")]
    pub bank_opening: Decimal,
    pub with_cash: bool,
    #[ts(type = "number")]
    pub cash_opening: Decimal,
    pub demo: bool,
}

pub async fn onboarding(State(st): State<AppState>, Json(input): Json<OnboardingInput>) -> ApiResult<SystemStatus> {
    st.write(|c| {
        let current = cfg::load(c)?;
        let mut s = input.settings;
        if s.tax.chart != current.tax.chart {
            cfg::change_chart(c, &s.tax.chart)?;
        }
        s.onboarded = true;
        s.tax.locked_until = current.tax.locked_until;
        if s.email.password.is_empty() {
            s.email.password = current.email.password.clone();
        }
        sanitize_refs(c, &mut s)?;
        if s.dunning.levels.is_empty() {
            s.dunning = DunningSettings::default();
        }
        if s.docs.texts.is_empty() {
            s.docs.texts = crate::services::defaults::default_texts();
        }
        cfg::save(c, &s)?;
        let opening_date = ymd(today().year(), 1, 1);
        let has_accounts = db::scalar_i64(c, "SELECT COUNT(*) FROM payment_accounts", &[])? > 0;
        if !has_accounts {
            finance::save_account(
                c,
                PaymentAccount {
                    name: if input.bank_name.trim().is_empty() { "Geschäftskonto".into() } else { input.bank_name.clone() },
                    kind: "bank".into(),
                    iban: if input.bank_iban.is_empty() { s.company.iban.clone() } else { input.bank_iban.clone() },
                    bic: s.company.bic.clone(),
                    bank_name: s.company.bank_name.clone(),
                    opening_balance: input.bank_opening,
                    opening_date,
                    color: "blue".into(),
                    ..Default::default()
                },
            )?;
            if input.with_cash {
                finance::save_account(
                    c,
                    PaymentAccount { name: "Kasse".into(), kind: "cash".into(), opening_balance: input.cash_opening, opening_date, color: "teal".into(), ..Default::default() },
                )?;
            }
        }
        if input.demo {
            demo::load(c)?;
        }
        Ok(())
    })?;
    status(State(st)).await
}

pub async fn demo(State(st): State<AppState>) -> ApiResult<SystemStatus> {
    st.write(|c| {
        let existing = db::scalar_i64(c, "SELECT (SELECT COUNT(*) FROM documents) + (SELECT COUNT(*) FROM receipts) + (SELECT COUNT(*) FROM contacts)", &[])?;
        if existing > 0 {
            crate::bail_user!("Demodaten können nur in einen leeren Datenbestand geladen werden. Bitte zuerst alle Daten zurücksetzen.");
        }
        if db::scalar_i64(c, "SELECT COUNT(*) FROM payment_accounts", &[])? == 0 {
            finance::save_account(
                c,
                PaymentAccount {
                    name: "Geschäftskonto".into(),
                    kind: "bank".into(),
                    opening_balance: Decimal::from(12500),
                    opening_date: ymd(today().year(), 1, 1),
                    color: "blue".into(),
                    ..Default::default()
                },
            )?;
        }
        demo::load(c)
    })?;
    status(State(st)).await
}

pub async fn rebuild(State(st): State<AppState>) -> ApiResult<super::MessageResponse> {
    st.write(|c| {
        posting::rebuild_all(c)?;
        let n = db::scalar_i64(c, "SELECT COUNT(*) FROM journal_entries", &[])?;
        Ok(super::MessageResponse { message: format!("{n} Buchungssätze neu erzeugt") })
    })
    .map(Json)
}

pub async fn backup(State(st): State<AppState>) -> Result<Response, AppError> {
    let zip = backup_zip(&st)?;
    let name = format!("Sorobo-Datensicherung-{}.zip", chrono::Local::now().format("%Y-%m-%d_%H%M"));
    Ok(file_response(zip, "application/zip", &name, true))
}

/// ZIP with a consistent copy of the database and all stored files.
fn backup_zip(st: &AppState) -> Result<Vec<u8>, AppError> {
    let mut buf = std::io::Cursor::new(Vec::new());
    write_backup(st, &mut buf)?;
    Ok(buf.into_inner())
}

fn write_backup<W: Write + std::io::Seek>(st: &AppState, out: W) -> Result<(), AppError> {
    let tmp = st.data_dir.join(format!("backup-{}.db", uuid::Uuid::new_v4()));
    {
        let conn = st.db();
        conn.execute("VACUUM INTO ?1", [tmp.display().to_string()]).map_err(AppError::from)?;
    }
    let result = (|| -> anyhow::Result<()> {
        let mut zip = zip::ZipWriter::new(out);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("sorobo.db", opts)?;
        std::io::copy(&mut std::fs::File::open(&tmp)?, &mut zip)?;
        if let Ok(entries) = std::fs::read_dir(st.files_dir()) {
            for e in entries.flatten() {
                if let Ok(mut f) = std::fs::File::open(e.path()) {
                    zip.start_file(format!("files/{}", e.file_name().to_string_lossy()), opts)?;
                    std::io::copy(&mut f, &mut zip)?;
                }
            }
        }
        zip.finish()?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    result.map_err(AppError::from)
}

const AUTO_BACKUP_PREFIX: &str = "Sorobo-Sicherung-";
/// Automatic backups made before the rename (until version 0.10 as "Sqonto") are rotated along.
const LEGACY_AUTO_BACKUP_PREFIX: &str = "Sqonto-Sicherung-";

fn backup_folder(st: &AppState, s: &Settings) -> std::path::PathBuf {
    let folder = s.backup.folder.trim();
    if folder.is_empty() { st.data_dir.join("backups") } else { std::path::PathBuf::from(folder) }
}

/// Date of an automatic backup from its file name (`2026-01-03` or `2026-01-03_120000`).
fn auto_backup_stamp(name: &str) -> Option<&str> {
    name.strip_prefix(AUTO_BACKUP_PREFIX).or_else(|| name.strip_prefix(LEGACY_AUTO_BACKUP_PREFIX))?.strip_suffix(".zip")
}

fn is_auto_backup(name: &str) -> bool {
    auto_backup_stamp(name).is_some()
}

/// Daily automatic backup into the backup folder (`force`: right now, even if today's exists).
/// Returns the written file, or None if nothing was due.
pub fn auto_backup(st: &AppState, force: bool) -> Result<Option<std::path::PathBuf>, AppError> {
    let s = st.read(cfg::load)?;
    if !force && (!s.backup.enabled || !s.onboarded) {
        return Ok(None);
    }
    let dir = backup_folder(st, &s);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::bad(format!("Der Sicherungsordner {} ist nicht verfügbar: {e}", dir.display())))?;
    let now = chrono::Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let today = format!("{AUTO_BACKUP_PREFIX}{date}");
    let legacy_today = format!("{LEGACY_AUTO_BACKUP_PREFIX}{date}");
    let exists = std::fs::read_dir(&dir).map_err(AppError::from)?.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        name.starts_with(&today) || name.starts_with(&legacy_today)
    });
    if exists && !force {
        return Ok(None);
    }
    let name = if exists { format!("{AUTO_BACKUP_PREFIX}{}.zip", now.format("%Y-%m-%d_%H%M%S")) } else { format!("{today}.zip") };
    let part = dir.join(format!("{name}.part"));
    let written = std::fs::File::create(&part).map_err(AppError::from).and_then(|f| write_backup(st, std::io::BufWriter::new(f)));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    let path = dir.join(&name);
    std::fs::rename(&part, &path).map_err(AppError::from)?;
    rotate_backups(&dir, s.backup.keep.max(1) as usize);
    let last = serde_json::json!({ "at": now_str(), "file": path.display().to_string() }).to_string();
    st.write(|c| db::meta_set(c, "last_backup", &last))?;
    Ok(Some(path))
}

/// Keeps the newest `keep` automatic backups; other files stay.
fn rotate_backups(dir: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| is_auto_backup(n)).collect();
    // By date, not by name: the name's prefix changed with the rename.
    names.sort_by(|a, b| auto_backup_stamp(a).cmp(&auto_backup_stamp(b)).then_with(|| a.cmp(b)));
    for n in names.iter().take(names.len().saturating_sub(keep)) {
        let _ = std::fs::remove_file(dir.join(n));
    }
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct BackupStatus {
    /// Folder the automatic backups go to.
    pub folder: String,
    pub default_folder: String,
    pub last_at: Option<String>,
    pub last_file: String,
    pub count: i64,
}

fn backup_status(st: &AppState) -> Result<BackupStatus, AppError> {
    let (s, last) = st.read(|c| Ok((cfg::load(c)?, db::meta_get(c, "last_backup")?)))?;
    let last: serde_json::Value = last.and_then(|l| serde_json::from_str(&l).ok()).unwrap_or_default();
    let dir = backup_folder(st, &s);
    let count = std::fs::read_dir(&dir).map(|e| e.flatten().filter(|e| is_auto_backup(&e.file_name().to_string_lossy())).count() as i64).unwrap_or(0);
    Ok(BackupStatus {
        folder: dir.display().to_string(),
        default_folder: st.data_dir.join("backups").display().to_string(),
        last_at: last["at"].as_str().map(str::to_string),
        last_file: last["file"].as_str().unwrap_or_default().to_string(),
        count,
    })
}

pub async fn backups(State(st): State<AppState>) -> ApiResult<BackupStatus> {
    backup_status(&st).map(Json)
}

pub async fn run_backup(State(st): State<AppState>) -> ApiResult<BackupStatus> {
    let worker = st.clone();
    tokio::task::spawn_blocking(move || auto_backup(&worker, true)).await.map_err(|e| AppError::from(anyhow::anyhow!(e)))??;
    backup_status(&st).map(Json)
}

/// Name of a stored file inside a backup (`files/<uuid>`). Stored files are always named by their
/// UUID, so anything else – paths, drive letters, device names – is rejected.
fn restore_file_name(entry: &str) -> Option<&str> {
    let name = entry.strip_prefix("files/")?;
    (name.len() == 36 && uuid::Uuid::try_parse(name).is_ok()).then_some(name)
}

/// Checks a restored database before it replaces the current one.
fn validate_backup_db(path: &std::path::Path) -> anyhow::Result<()> {
    let conn = rusqlite::Connection::open(path)?;
    let check: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if check != "ok" {
        anyhow::bail!("Integritätsprüfung: {check}");
    }
    for table in ["meta", "accounts", "documents", "journal_entries"] {
        conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |r| r.get::<_, i64>(0))
            .ok()
            .filter(|n| *n == 1)
            .ok_or_else(|| anyhow::anyhow!("Tabelle {table} fehlt"))?;
    }
    if let Some(v) = db::schema_version(&conn)
        && v > db::SCHEMA_VERSION
    {
        anyhow::bail!("Sicherung einer neueren Sorobo-Version (Schema {v}) – bitte zuerst Sorobo aktualisieren");
    }
    Ok(())
}

pub async fn restore(State(st): State<AppState>, mut mp: Multipart) -> ApiResult<SystemStatus> {
    let mut bytes = None;
    while let Some(field) = mp.next_field().await.map_err(|e| AppError::bad(e.to_string()))? {
        if field.name() == Some("file") {
            bytes = Some(field.bytes().await.map_err(|e| AppError::bad(e.to_string()))?.to_vec());
        }
    }
    let bytes = bytes.ok_or_else(|| AppError::bad("Keine Datei übermittelt"))?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| AppError::bad("Die Datei ist keine gültige Sorobo-Datensicherung (ZIP)."))?;
    let mut db_bytes = Vec::new();
    // Backups from before the renames contain "sqonto.db" (until version 0.10) or "kontor.db".
    let db_index = ["sorobo.db", "sqonto.db", "kontor.db"]
        .into_iter()
        .find_map(|name| archive.index_for_name(name))
        .ok_or_else(|| AppError::bad("Die Sicherung enthält keine Datenbank (sorobo.db)."))?;
    archive.by_index(db_index).map_err(AppError::from)?.read_to_end(&mut db_bytes).map_err(AppError::from)?;
    if !db_bytes.starts_with(b"SQLite format 3") {
        return Err(AppError::bad("Die Datenbank in der Sicherung ist beschädigt."));
    }
    // Database and files are staged next to the current data; nothing is replaced before both
    // are complete and the database has been checked.
    let staged_db = st.data_dir.join("sorobo.db.restore");
    let staged_files = st.data_dir.join("files.restore");
    let cleanup = || {
        let _ = std::fs::remove_file(&staged_db);
        let _ = std::fs::remove_dir_all(&staged_files);
    };
    cleanup();
    if let Err(e) = stage_restore(&mut archive, &db_bytes, &staged_db, &staged_files) {
        cleanup();
        return Err(AppError::from(e));
    }
    // The current state is kept in data/backups in case the wrong backup was chosen.
    let safety_dir = st.data_dir.join("backups");
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H%M%S").to_string();
    let safety = (1..)
        .map(|n| safety_dir.join(if n == 1 { format!("Sorobo-vor-Wiederherstellung-{stamp}.zip") } else { format!("Sorobo-vor-Wiederherstellung-{stamp}-{n}.zip") }))
        .find(|p| !p.exists())
        .unwrap_or_default();
    let saved = std::fs::create_dir_all(&safety_dir).map_err(AppError::from).and_then(|_| backup_zip(&st)).and_then(|z| std::fs::write(&safety, z).map_err(AppError::from));
    if let Err(e) = saved {
        cleanup();
        return Err(AppError::bad(format!("Der aktuelle Datenstand konnte nicht gesichert werden, die Wiederherstellung wurde abgebrochen: {}", e.message)));
    }
    swap_in_restore(&st, &staged_db, &staged_files).map_err(|e| {
        cleanup();
        AppError::from(anyhow::anyhow!("Wiederherstellung fehlgeschlagen ({e:#}). Der vorherige Datenstand liegt in {}.", safety.display()))
    })?;
    tracing::info!("Datensicherung wiederhergestellt, vorheriger Stand: {}", safety.display());
    status(State(st)).await
}

/// Writes the backup's database and stored files to the staging locations and checks them.
fn stage_restore(archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, db_bytes: &[u8], staged_db: &std::path::Path, staged_files: &std::path::Path) -> anyhow::Result<()> {
    std::fs::write(staged_db, db_bytes)?;
    validate_backup_db(staged_db).map_err(|e| crate::error::UserError(format!("Die Datenbank in der Sicherung ist beschädigt ({e}).")))?;
    std::fs::create_dir_all(staged_files)?;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i)?;
        let entry = f.name().map(|n| n.to_string()).unwrap_or_default();
        if !entry.starts_with("files/") || entry.ends_with('/') {
            continue;
        }
        let Some(name) = restore_file_name(&entry) else {
            tracing::warn!("Wiederherstellung: Eintrag {entry:?} übersprungen");
            continue;
        };
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        std::fs::write(staged_files.join(name), data)?;
    }
    Ok(())
}

/// Replaces database and files by the staged ones; on failure the previous state is put back.
fn swap_in_restore(st: &AppState, staged_db: &std::path::Path, staged_files: &std::path::Path) -> anyhow::Result<()> {
    let db_path = st.data_dir.join("sorobo.db");
    let files_dir = st.files_dir();
    let previous_files = st.data_dir.join("files.previous");
    let _ = std::fs::remove_dir_all(&previous_files);
    if files_dir.exists() {
        std::fs::rename(&files_dir, &previous_files)?;
    }
    let put_back_files = || {
        let _ = std::fs::remove_dir_all(&files_dir);
        let _ = std::fs::rename(&previous_files, &files_dir);
    };
    if let Err(e) = std::fs::rename(staged_files, &files_dir) {
        put_back_files();
        return Err(e.into());
    }
    let mut guard = st.db();
    *guard = rusqlite::Connection::open_in_memory()?;
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(st.data_dir.join(format!("sorobo.db{suffix}")));
    }
    let opened = std::fs::rename(staged_db, &db_path).map_err(anyhow::Error::from).and_then(|_| db::open(&db_path));
    match opened {
        Ok(conn) => *guard = conn,
        Err(e) => {
            put_back_files();
            *guard = db::open(&db_path)?;
            return Err(e);
        }
    }
    cfg::ensure_seeded(&guard)?;
    drop(guard);
    let _ = std::fs::remove_dir_all(&previous_files);
    Ok(())
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct ResetRequest {
    pub confirm: String,
}

/// Every table of the schema except `meta`, children before the tables they reference.
const WIPED_TABLES: &[&str] = &[
    "document_archive", "debit_run_items", "debit_runs", "mandates", "payment_run_items", "payment_runs", "recurring_invoices", "bank_rules", "journal_lines", "journal_entries", "allocations", "transactions", "dunnings", "document_lines", "time_entries", "documents", "people", "receipt_lines",
    "receipts", "assets", "manual_entries", "payment_accounts", "articles", "contacts", "activities", "ustva_filings", "zm_filings", "files",
    "number_ranges", "layouts", "accounts",
];

/// "Alle Daten löschen": empties the database and seeds the defaults again.
fn wipe_data(c: &rusqlite::Connection) -> anyhow::Result<()> {
    for t in WIPED_TABLES {
        c.execute(&format!("DELETE FROM {t}"), [])?;
    }
    c.execute("DELETE FROM meta WHERE key != 'schema_version'", [])?;
    cfg::ensure_seeded(c)?;
    Ok(())
}

pub async fn reset(State(st): State<AppState>, Json(req): Json<ResetRequest>) -> ApiResult<SystemStatus> {
    if req.confirm != "LÖSCHEN" {
        return Err(AppError::bad("Bitte zur Bestätigung LÖSCHEN eingeben."));
    }
    st.write(wipe_data)?;
    let _ = std::fs::remove_dir_all(st.files_dir());
    let _ = std::fs::create_dir_all(st.files_dir());
    status(State(st)).await
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct FontList {
    pub recommended: Vec<String>,
    pub all: Vec<String>,
}

pub async fn fonts(State(st): State<AppState>) -> ApiResult<FontList> {
    let all = st.pdf.families().to_vec();
    let recommended = PdfEngine::bundled_families().into_iter().filter(|f| all.iter().any(|a| a == f)).map(String::from).collect();
    Ok(Json(FontList { recommended, all }))
}

#[derive(Deserialize)]
pub struct SearchQ {
    #[serde(default)]
    q: String,
}

pub async fn search(State(st): State<AppState>, Query(q): Query<SearchQ>) -> ApiResult<Vec<masterdata::SearchHit>> {
    st.read(|c| masterdata::search(c, &q.q)).map(Json)
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct ActivityItem {
    pub activity: Activity,
    pub title: String,
    pub link: String,
}

pub async fn activities(State(st): State<AppState>) -> ApiResult<Vec<ActivityItem>> {
    st.read(|c| {
        let mut out = Vec::new();
        for a in db::query::<Activity>(c, "ORDER BY id DESC LIMIT 30", &[])? {
            let (title, link) = match a.entity.as_str() {
                "document" => match db::find::<Document>(c, a.entity_id)? {
                    Some(d) => (
                        format!("{} {}", crate::services::documents::type_label(&d.doc_type, &d.kind), if d.number.is_empty() { "Entwurf".into() } else { d.number }),
                        format!("/dokumente/{}", d.id),
                    ),
                    None => continue,
                },
                "receipt" => match db::find::<Receipt>(c, a.entity_id)? {
                    Some(r) => (format!("Beleg {}", r.number), format!("/belege/{}", r.id)),
                    None => continue,
                },
                "contact" => match db::find::<Contact>(c, a.entity_id)? {
                    Some(k) => (k.display_name(), format!("/kontakte/{}", k.id)),
                    None => continue,
                },
                // Kassenbuch changes are kept even when the entry itself was deleted.
                "transaction" => ("Kassenbuch".into(), "/kasse".into()),
                _ => (a.entity.clone(), String::new()),
            };
            out.push(ActivityItem { activity: a, title, link });
        }
        Ok(out)
    })
    .map(Json)
}

pub async fn upload(State(st): State<AppState>, mut mp: Multipart) -> ApiResult<FileMeta> {
    while let Some(field) = mp.next_field().await.map_err(|e| AppError::bad(e.to_string()))? {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field.file_name().unwrap_or("upload").to_string();
        let mut mime = field.content_type().unwrap_or("application/octet-stream").to_string();
        if mime == "application/octet-stream" {
            mime = mime_guess::from_path(&filename).first_or_octet_stream().to_string();
        }
        let bytes = field.bytes().await.map_err(|e| AppError::bad(e.to_string()))?;
        return save_file(&st, &filename, &mime, &bytes).map(Json);
    }
    Err(AppError::bad("Keine Datei übermittelt"))
}

pub fn save_file(st: &AppState, filename: &str, mime: &str, bytes: &[u8]) -> Result<FileMeta, AppError> {
    let uuid = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(st.files_dir()).map_err(AppError::from)?;
    std::fs::write(st.files_dir().join(&uuid), bytes).map_err(AppError::from)?;
    let mut meta = FileMeta { id: 0, uuid, filename: filename.to_string(), mime: mime.to_string(), size: bytes.len() as i64, created_at: now_str() };
    st.write(|c| db::insert(c, &mut meta))?;
    Ok(meta)
}

#[derive(Deserialize)]
pub struct FileQ {
    #[serde(default)]
    download: bool,
}

pub async fn file(State(st): State<AppState>, Path(id): Path<i64>, Query(q): Query<FileQ>) -> Result<Response, AppError> {
    let meta: FileMeta = st.read(|c| db::get(c, id))?;
    let bytes = std::fs::read(st.files_dir().join(&meta.uuid)).map_err(|_| AppError::not_found("Datei nicht gefunden"))?;
    let mut resp = file_response(bytes, &meta.mime, &meta.filename, q.download);
    sandbox_upload(&mut resp, &meta.mime);
    Ok(resp)
}

/// Uploaded files come from third parties (receipts, e-invoices) and carry the MIME type the
/// uploader claimed. The browser must not sniff another type, and everything except PDFs (shown by
/// the browser's PDF viewer) is sandboxed: an HTML or SVG file opened in the app can neither run
/// scripts nor act on the app's origin.
fn sandbox_upload(resp: &mut Response, mime: &str) {
    let h = resp.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if !mime.trim().eq_ignore_ascii_case("application/pdf") {
        h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; img-src data:; style-src 'unsafe-inline'; sandbox"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(mime: &str) -> (Option<String>, Option<String>) {
        let mut resp = file_response(b"x".to_vec(), mime, "datei", false);
        sandbox_upload(&mut resp, mime);
        let get = |n: header::HeaderName| resp.headers().get(n).map(|v| v.to_str().unwrap().to_string());
        (get(header::X_CONTENT_TYPE_OPTIONS), get(header::CONTENT_SECURITY_POLICY))
    }

    #[test]
    fn the_reset_covers_every_table_and_works_with_hours_booked() {
        let conn = db::open_memory();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != 'meta'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for t in &tables {
            assert!(WIPED_TABLES.contains(&t.as_str()), "the reset does not clear {t}");
        }
        cfg::ensure_seeded(&conn).unwrap();
        conn.execute("INSERT INTO contacts (company) VALUES ('Kunde GmbH')", []).unwrap();
        conn.execute("INSERT INTO documents (doc_type, date, number) VALUES ('quote', '2026-01-10', 'AN-TEST')", []).unwrap();
        let doc = conn.last_insert_rowid();
        let person = crate::services::timetracking::save_person(&conn, Person { name: "Anna".into(), ..Default::default() }).unwrap();
        let entry = TimeEntry { person_id: person.id, document_id: doc, date: "2026-01-12".parse().unwrap(), hours: rust_decimal_macros::dec!(2), ..Default::default() };
        crate::services::timetracking::save_entry(&conn, entry).unwrap();
        wipe_data(&conn).unwrap();
        for t in ["documents", "people", "time_entries"] {
            assert_eq!(db::scalar_i64(&conn, &format!("SELECT COUNT(*) FROM {t}"), &[]).unwrap(), 0, "{t}");
        }
    }

    #[test]
    fn rotation_keeps_the_newest_automatic_backups() {
        let dir = std::env::temp_dir().join(format!("sorobo-rotate-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for d in ["2026-01-01", "2026-01-02", "2026-01-03", "2026-01-03_120000", "2026-01-04"] {
            std::fs::write(dir.join(format!("{AUTO_BACKUP_PREFIX}{d}.zip")), b"x").unwrap();
        }
        std::fs::write(dir.join("Sorobo-vor-Wiederherstellung-2025-12-01_100000.zip"), b"x").unwrap();
        std::fs::write(dir.join("notizen.txt"), b"x").unwrap();
        rotate_backups(&dir, 3);
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            left,
            vec![
                "Sorobo-Sicherung-2026-01-03.zip",
                "Sorobo-Sicherung-2026-01-03_120000.zip",
                "Sorobo-Sicherung-2026-01-04.zip",
                "Sorobo-vor-Wiederherstellung-2025-12-01_100000.zip",
                "notizen.txt"
            ]
        );
    }

    #[test]
    fn rotation_orders_backups_from_before_the_rename_by_date() {
        let dir = std::env::temp_dir().join(format!("sorobo-rotate-legacy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // "Sqonto-…" sorts after "Sorobo-…" by name, so the date decides.
        for d in ["2026-01-02", "2026-01-05"] {
            std::fs::write(dir.join(format!("{LEGACY_AUTO_BACKUP_PREFIX}{d}.zip")), b"x").unwrap();
        }
        for d in ["2026-01-01", "2026-01-03", "2026-01-04"] {
            std::fs::write(dir.join(format!("{AUTO_BACKUP_PREFIX}{d}.zip")), b"x").unwrap();
        }
        std::fs::write(dir.join("Sqonto-vor-Wiederherstellung-2025-12-01_100000.zip"), b"x").unwrap();
        rotate_backups(&dir, 3);
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            left,
            vec!["Sorobo-Sicherung-2026-01-03.zip", "Sorobo-Sicherung-2026-01-04.zip", "Sqonto-Sicherung-2026-01-05.zip", "Sqonto-vor-Wiederherstellung-2025-12-01_100000.zip"]
        );
    }

    #[test]
    fn restore_accepts_only_stored_file_names() {
        let id = "0b9c3a52-6a3e-4a8e-9a4f-2f2b7c1d9e10";
        assert_eq!(restore_file_name(&format!("files/{id}")), Some(id));
        for bad in [
            "files/",
            "files/../sorobo.db",
            "files/..\\..\\evil.exe",
            "files/C:\\Windows\\evil.dll",
            "files/sub/0b9c3a52-6a3e-4a8e-9a4f-2f2b7c1d9e10",
            "files/CON",
            "files/0b9c3a526a3e4a8e9a4f2f2b7c1d9e10",
            "sorobo.db",
        ] {
            assert_eq!(restore_file_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn uploads_are_sandboxed() {
        for mime in ["text/html", "image/svg+xml", "application/xml", "image/png", "application/octet-stream", "text/html; charset=utf-8"] {
            let (nosniff, csp) = headers(mime);
            assert_eq!(nosniff.as_deref(), Some("nosniff"), "{mime}");
            assert!(csp.is_some_and(|c| c.contains("sandbox") && c.contains("default-src 'none'")), "{mime}");
        }
        // PDFs stay viewable in the browser's PDF viewer (iframe in the receipt editor).
        assert_eq!(headers("application/pdf"), (Some("nosniff".into()), None));
    }
}

// ---------------------------------------------------------------- Layouts

pub async fn layouts(State(st): State<AppState>) -> ApiResult<Vec<Layout>> {
    st.read(|c| db::query::<Layout>(c, "ORDER BY is_default DESC, name", &[])).map(Json)
}

pub async fn layout(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Layout> {
    st.read(|c| db::get(c, id)).map(Json)
}

pub async fn create_layout(State(st): State<AppState>, Json(mut l): Json<Layout>) -> ApiResult<Layout> {
    st.write(|c| {
        l.id = 0;
        if l.name.trim().is_empty() {
            l.name = "Neues Layout".into();
        }
        l.is_default = db::scalar_i64(c, "SELECT COUNT(*) FROM layouts", &[])? == 0;
        l.created_at = now_str();
        l.updated_at = now_str();
        db::insert(c, &mut l)?;
        Ok(l)
    })
    .map(Json)
}

pub async fn update_layout(State(st): State<AppState>, Path(id): Path<i64>, Json(mut l): Json<Layout>) -> ApiResult<Layout> {
    st.write(|c| {
        let old: Layout = db::get(c, id)?;
        l.id = id;
        l.is_default = old.is_default;
        l.created_at = old.created_at;
        l.updated_at = now_str();
        db::update(c, &l)?;
        Ok(l)
    })
    .map(Json)
}

pub async fn delete_layout(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<super::MessageResponse> {
    st.write(|c| {
        if db::scalar_i64(c, "SELECT COUNT(*) FROM layouts", &[])? <= 1 {
            crate::bail_user!("Das letzte Layout kann nicht gelöscht werden.");
        }
        let l: Layout = db::get(c, id)?;
        c.execute("UPDATE documents SET layout_id = NULL WHERE layout_id = ?1", [id])?;
        db::delete::<Layout>(c, id)?;
        if l.is_default {
            c.execute("UPDATE layouts SET is_default = 1 WHERE id = (SELECT MIN(id) FROM layouts)", [])?;
        }
        Ok(super::MessageResponse { message: "Layout gelöscht".into() })
    })
    .map(Json)
}

pub async fn default_layout(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Vec<Layout>> {
    st.write(|c| {
        let _: Layout = db::get(c, id)?;
        c.execute("UPDATE layouts SET is_default = (id = ?1)", [id])?;
        let mut s = cfg::load(c)?;
        s.docs.default_layout_id = Some(id);
        cfg::save(c, &s)?;
        db::query::<Layout>(c, "ORDER BY is_default DESC, name", &[])
    })
    .map(Json)
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct LayoutPreviewRequest {
    pub config: LayoutConfig,
    pub document_id: Option<i64>,
    /// invoice | quote | delivery | dunning
    pub doc_type: Option<String>,
}

pub async fn layout_preview(State(st): State<AppState>, Json(req): Json<LayoutPreviewRequest>) -> ApiResult<Pages> {
    let job = st.read(|c| {
        let (mut doc, lines) = match req.document_id {
            Some(id) => (db::get::<Document>(c, id)?, crate::services::documents::lines_of(c, id)?),
            None => data::sample_document(),
        };
        if let Some(t) = &req.doc_type
            && req.document_id.is_none()
        {
            doc.doc_type = t.clone();
            doc.title = crate::services::documents::type_label(t, "standard").into();
            if t == "quote" {
                doc.number = "AN-2026-0017".into();
                doc.valid_until = Some(today() + chrono::Duration::days(30));
                doc.due_date = None;
            } else if t == "delivery" {
                doc.number = "LS-2026-0008".into();
            }
        }
        data::document_job(c, &st.data_dir, &doc, &lines, Some(req.config.clone()))
    })?;
    Ok(Json(Pages { pages: render_svgs(&st, job).await? }))
}

pub async fn number_ranges(State(st): State<AppState>) -> ApiResult<Vec<NumberRange>> {
    st.read(|c| db::query::<NumberRange>(c, "ORDER BY id", &[])).map(Json)
}

pub async fn save_number_ranges(State(st): State<AppState>, Json(ranges): Json<Vec<NumberRange>>) -> ApiResult<Vec<NumberRange>> {
    st.write(|c| {
        for r in ranges {
            if !r.format.contains("{N") {
                crate::bail_user!("Das Format „{}“ muss einen Zähler wie {{NNNN}} enthalten.", r.format);
            }
            let mut old = db::query_one::<NumberRange>(c, "WHERE key = ?1", &[v(r.key.clone())])?
                .ok_or_else(|| anyhow::anyhow!("Unbekannter Nummernkreis {}", r.key))?;
            old.format = r.format;
            old.next = r.next.max(1);
            old.reset_yearly = r.reset_yearly;
            db::update(c, &old)?;
        }
        db::query::<NumberRange>(c, "ORDER BY id", &[])
    })
    .map(Json)
}
