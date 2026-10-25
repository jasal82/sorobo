// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! HTTP API (JSON under /api) and static frontend.

mod accounting;
mod documents;
mod finance;
mod masterdata;
mod reports;
mod system;
mod timetracking;
mod update;

pub(crate) use documents::deliver_recurring;
pub(crate) use system::auto_backup;

use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use rust_embed::RustEmbed;

use crate::error::AppError;
use crate::pdf::data::RenderJob;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        // System & Einstellungen
        .route("/system/status", get(system::status))
        .route("/system/focus", post(system::focus))
        .route("/system/onboarding", post(system::onboarding))
        .route("/system/demo", post(system::demo))
        .route("/system/rebuild", post(system::rebuild))
        .route("/system/backup", get(system::backup))
        .route("/system/backups", get(system::backups).post(system::run_backup))
        .route("/system/restore", post(system::restore))
        .route("/system/reset", post(system::reset))
        .route("/system/update", get(update::status))
        .route("/system/update/check", post(update::check))
        .route("/system/update/install", post(update::install))
        .route("/system/update/skip", post(update::skip))
        .route("/settings", get(system::get_settings).put(system::put_settings))
        .route("/settings/getting-started", put(system::put_getting_started))
        .route("/settings/lock", post(system::lock_period))
        .route("/settings/email-test", post(system::email_test))
        .route("/fonts", get(system::fonts))
        .route("/search", get(system::search))
        .route("/activities", get(system::activities))
        .route("/files", post(system::upload))
        .route("/files/{id}", get(system::file))
        .route("/layouts", get(system::layouts).post(system::create_layout))
        .route("/layouts/preview", post(system::layout_preview))
        .route("/layouts/{id}", get(system::layout).put(system::update_layout).delete(system::delete_layout))
        .route("/layouts/{id}/default", post(system::default_layout))
        .route("/number-ranges", get(system::number_ranges).put(system::save_number_ranges))
        // Stammdaten
        .route("/contacts", get(masterdata::contacts).post(masterdata::create_contact))
        .route("/contacts/{id}", get(masterdata::contact).put(masterdata::update_contact).delete(masterdata::delete_contact))
        .route("/contacts/{id}/defaults", get(masterdata::contact_defaults))
        .route("/articles", get(masterdata::articles).post(masterdata::create_article))
        .route("/articles/{id}", get(masterdata::article).put(masterdata::update_article).delete(masterdata::delete_article))
        // Zeiterfassung
        .route("/people", get(timetracking::people).post(timetracking::create_person))
        .route("/people/{id}", put(timetracking::update_person).delete(timetracking::delete_person))
        .route("/time-entries", get(timetracking::entries).post(timetracking::create_entry))
        .route("/time-entries/report", get(timetracking::report))
        .route("/time-entries/{id}", put(timetracking::update_entry).delete(timetracking::delete_entry))
        // Dokumente
        .route("/documents", get(documents::list).post(documents::create))
        .route("/documents/counts", get(documents::counts))
        .route("/documents/new", get(documents::new_template))
        .route("/documents/preview", post(documents::preview_input))
        .route("/documents/{id}", get(documents::get).put(documents::update).delete(documents::delete))
        .route("/documents/{id}/finalize", post(documents::finalize))
        .route("/documents/{id}/status", post(documents::set_status))
        .route("/documents/{id}/sent", post(documents::mark_sent))
        .route("/documents/{id}/email", get(documents::email_draft).post(documents::send_email))
        .route("/documents/{id}/copy", post(documents::copy))
        .route("/documents/{id}/convert", post(documents::convert))
        .route("/documents/{id}/cancel", post(documents::cancel))
        .route("/documents/{id}/credit-note", post(documents::credit_note))
        .route("/documents/{id}/partial", post(documents::partial))
        .route("/documents/{id}/final", post(documents::final_invoice))
        .route("/documents/{id}/payments", post(documents::pay))
        .route("/documents/{id}/writeoff", post(documents::writeoff))
        .route("/documents/{id}/pdf", get(documents::pdf))
        .route("/documents/{id}/preview", get(documents::preview))
        .route("/documents/{id}/xrechnung", get(documents::xrechnung))
        .route("/documents/{id}/zugferd", get(documents::zugferd))
        .route("/documents/{id}/einvoice-check", get(documents::einvoice_check))
        .route("/recurring", get(documents::recurring).post(documents::save_recurring))
        .route("/recurring/{id}", put(documents::save_recurring).delete(documents::delete_recurring))
        .route("/dunning", get(documents::dunnings).post(documents::create_dunning))
        .route("/dunning/suggestions", get(documents::dunning_suggestions))
        .route("/dunning/preview", post(documents::dunning_preview))
        .route("/dunning/{id}", delete(documents::delete_dunning))
        .route("/dunning/{id}/pdf", get(documents::dunning_pdf))
        // Belege
        .route("/receipts", get(finance::receipts).post(finance::create_receipt))
        .route("/receipts/import", post(finance::import_einvoice))
        .route("/receipts/suggest", get(finance::suggest_account))
        .route("/receipts/{id}", get(finance::receipt).put(finance::update_receipt).delete(finance::delete_receipt))
        .route("/receipts/{id}/payments", post(finance::pay_receipt))
        // Zahlungsverkehr
        .route("/payment-accounts", get(finance::payment_accounts).post(finance::create_payment_account))
        .route("/payment-accounts/{id}", put(finance::update_payment_account).delete(finance::delete_payment_account))
        .route("/transactions", get(finance::transactions).post(finance::create_transaction))
        .route("/transactions/{id}", put(finance::update_transaction).delete(finance::delete_transaction))
        .route("/transactions/{id}/suggestions", get(finance::suggestions))
        .route("/allocations/{id}", delete(finance::delete_allocation))
        .route("/bank-import/preview", post(finance::import_preview))
        .route("/bank-import", post(finance::import_transactions))
        .route("/bank-rules", get(finance::bank_rules).post(finance::create_bank_rule))
        .route("/bank-rules/apply", post(finance::apply_bank_rules))
        .route("/bank-rules/{id}", put(finance::update_bank_rule).delete(finance::delete_bank_rule))
        .route("/payment-runs", get(finance::payment_runs).post(finance::create_payment_run))
        .route("/payment-runs/candidates", get(finance::transfer_candidates))
        .route("/payment-runs/{id}", delete(finance::delete_payment_run))
        .route("/payment-runs/{id}/xml", get(finance::payment_run_file))
        .route("/mandates", get(finance::mandates).post(finance::save_mandate))
        .route("/mandates/reference", get(finance::mandate_reference))
        .route("/mandates/{id}", put(finance::save_mandate).delete(finance::delete_mandate))
        .route("/mandates/{id}/pdf", get(finance::mandate_pdf))
        .route("/debit-runs", get(finance::debit_runs).post(finance::create_debit_run))
        .route("/debit-runs/candidates", get(finance::debit_candidates))
        .route("/debit-runs/{id}", delete(finance::delete_debit_run))
        .route("/debit-runs/{id}/xml", get(finance::debit_run_file))
        .route("/cashbook", get(finance::cashbook).post(finance::create_cash_entry))
        .route("/cashbook/pdf", get(finance::cashbook_pdf))
        .route("/cashbook/{id}/reverse", post(finance::reverse_cash_entry))
        // Buchhaltung
        .route("/accounts", get(accounting::accounts).post(accounting::save_account))
        .route("/accounts/mappings", get(accounting::account_mappings))
        .route("/accounts/{id}", put(accounting::save_account))
        .route("/journal", get(accounting::journal))
        .route("/ledger/{number}", get(accounting::ledger))
        .route("/manual-entries", get(accounting::manual_entries).post(accounting::save_manual_entry))
        .route("/manual-entries/{id}", put(accounting::save_manual_entry).delete(accounting::delete_manual_entry))
        .route("/assets", get(accounting::assets).post(accounting::save_asset))
        .route("/assets/{id}", get(accounting::asset).put(accounting::save_asset).delete(accounting::delete_asset))
        // Berichte
        .route("/dashboard", get(reports::dashboard))
        .route("/reports/analytics", get(reports::analytics))
        .route("/reports/euer", get(reports::euer))
        .route("/reports/guv", get(reports::guv))
        .route("/reports/bilanz", get(reports::bilanz))
        .route("/reports/bwa", get(reports::bwa))
        .route("/reports/susa", get(reports::susa))
        .route("/reports/open-items", get(reports::open_items))
        .route("/reports/ustva", get(reports::ustva_periods))
        .route("/reports/ustva/{year}/{period}", get(reports::ustva))
        .route("/reports/ustva/{year}/{period}/filing", post(reports::file_ustva).delete(reports::unfile_ustva))
        .route("/reports/zm", get(reports::zm_periods))
        .route("/reports/zm/{year}/{period}", get(reports::zm_report))
        .route("/reports/zm/{year}/{period}/filing", post(reports::file_zm).delete(reports::unfile_zm))
        .route("/reports/zm/{year}/{period}/csv", get(reports::zm_csv))
        .route("/reports/pdf/{kind}", get(reports::report_pdf))
        .route("/exports/datev", get(reports::datev))
        .route("/exports/receipts", get(reports::receipts_zip))
        .route("/exports/gdpdu", get(reports::gdpdu_export))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(state);
    Router::new().nest("/api", api).fallback(static_handler)
}

#[derive(RustEmbed)]
#[folder = "../frontend/dist/"]
// A fresh checkout has no built frontend yet (development uses the Vite dev server).
#[allow_missing = true]
struct Frontend;

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Frontend::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
            ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())], file.data).into_response()
        }
        None => match Frontend::get("index.html") {
            Some(index) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string())], index.data).into_response(),
            None => (StatusCode::NOT_FOUND, "Frontend nicht gebaut. Bitte `npm run build` im Ordner frontend ausführen.").into_response(),
        },
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn file_response(bytes: Vec<u8>, mime: &str, filename: &str, download: bool) -> Response {
    let disposition = format!(
        "{}; filename=\"{}\"; filename*=UTF-8''{}",
        if download { "attachment" } else { "inline" },
        filename.replace(['"', '\\'], "_").chars().map(|c| if c.is_ascii() { c } else { '_' }).collect::<String>(),
        urlencode(filename)
    );
    let mut resp = Response::new(Body::from(bytes));
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn safe_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' };
        // Collapse runs like "Design & Entwicklung" -> "Design_Entwicklung".
        if !(c == '_' && out.ends_with('_')) {
            out.push(c);
        }
    }
    out.trim_matches('_').replace("_.", ".")
}

/// Compiles a render job to PDF on a blocking thread.
pub async fn render_pdf(st: &AppState, job: RenderJob, pdfa: bool) -> Result<Vec<u8>, AppError> {
    let st = st.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let doc = st.pdf.compile(job.source, job.files)?;
        st.pdf.pdf(&doc, pdfa)
    })
    .await
    .map_err(|e| AppError::from(anyhow::anyhow!(e)))?
    .map_err(AppError::from)
}

/// Compiles a render job once to PDF and SVG pages (identical content for download and preview).
pub async fn render_pdf_and_svgs(st: &AppState, job: RenderJob, pdfa: bool) -> Result<(Vec<u8>, Vec<String>), AppError> {
    let st = st.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<(Vec<u8>, Vec<String>)> {
        let doc = st.pdf.compile(job.source, job.files)?;
        Ok((st.pdf.pdf(&doc, pdfa)?, st.pdf.svgs(&doc)))
    })
    .await
    .map_err(|e| AppError::from(anyhow::anyhow!(e)))?
    .map_err(AppError::from)
}

/// Compiles a render job to SVG pages.
pub async fn render_svgs(st: &AppState, job: RenderJob) -> Result<Vec<String>, AppError> {
    let st = st.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
        let doc = st.pdf.compile(job.source, job.files)?;
        Ok(st.pdf.svgs(&doc))
    })
    .await
    .map_err(|e| AppError::from(anyhow::anyhow!(e)))?
    .map_err(AppError::from)
}

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct Pages {
    pub pages: Vec<String>,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct IdResponse {
    pub id: i64,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct MessageResponse {
    pub message: String,
}
