// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Update check and self-update (see `services::update`).

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use ts_rs::TS;

use crate::error::{ApiResult, AppError};
use crate::services::settings as cfg;
use crate::services::update::{self, Phase, UpdateStatus};
use crate::state::AppState;

pub async fn status() -> Json<UpdateStatus> {
    Json(update::status())
}

pub async fn check() -> ApiResult<UpdateStatus> {
    tokio::task::spawn_blocking(update::check).await.map(Json).map_err(|e| AppError::from(anyhow::anyhow!(e)))
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct SkipRequest {
    pub version: String,
}

/// No more reminders for this version (it stays visible in the settings).
pub async fn skip(State(st): State<AppState>, Json(req): Json<SkipRequest>) -> ApiResult<UpdateStatus> {
    st.write(|c| {
        let mut s = cfg::load(c)?;
        s.updates.skipped_version = req.version;
        cfg::save(c, &s)
    })?;
    Ok(Json(update::status()))
}

/// Starts the installation in the background; the page follows it via `GET /system/update`.
pub async fn install(State(st): State<AppState>) -> ApiResult<UpdateStatus> {
    update::begin().map_err(AppError::bad)?;
    std::thread::spawn(move || run_install(st));
    Ok(Json(update::status()))
}

fn run_install(st: AppState) {
    // A safety copy first: the new version may update the database, which an older version then
    // no longer opens.
    let prepared = super::auto_backup(&st, true)
        .map_err(|e| format!("Die Sicherung vor dem Update ist fehlgeschlagen: {}", e.message))
        .and_then(|_| update::install(&st.updates_dir()).map_err(|e| format!("{e:#}")));
    let mut command = match prepared {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Update fehlgeschlagen: {e}");
            update::fail(e);
            return;
        }
    };
    update::set_phase(Phase::Restarting);
    // Give the page a moment to show the restart; no database write may be cut off.
    std::thread::sleep(Duration::from_millis(800));
    let _db = st.db();
    if let Err(e) = command.spawn() {
        update::fail(format!("{} konnte nicht gestartet werden ({e}). Bitte Sorobo beenden und neu starten.", command.get_program().to_string_lossy()));
        return;
    }
    tracing::info!("Update wird abgeschlossen, Sorobo startet neu");
    crate::quit();
    // The desktop app ends from its event loop; should it not, end here.
    std::thread::sleep(Duration::from_secs(10));
    std::process::exit(0);
}
