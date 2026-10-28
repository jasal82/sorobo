// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

use axum::Json;
use axum::extract::{Path, Query, State};

use super::MessageResponse;
use crate::error::ApiResult;
use crate::models::{Person, TimeEntry};
use crate::services::timetracking::{self as tt, TimeEntryItem, TimeFilter, TimeReport};
use crate::state::AppState;

pub async fn people(State(st): State<AppState>) -> ApiResult<Vec<Person>> {
    st.read(tt::list_people).map(Json)
}

pub async fn create_person(State(st): State<AppState>, Json(mut p): Json<Person>) -> ApiResult<Person> {
    p.id = 0;
    st.write(|c| tt::save_person(c, p)).map(Json)
}

pub async fn update_person(State(st): State<AppState>, Path(id): Path<i64>, Json(mut p): Json<Person>) -> ApiResult<Person> {
    p.id = id;
    st.write(|c| tt::save_person(c, p)).map(Json)
}

pub async fn delete_person(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| tt::delete_person(c, id)).map(|message| Json(MessageResponse { message }))
}

pub async fn entries(State(st): State<AppState>, Query(f): Query<TimeFilter>) -> ApiResult<Vec<TimeEntryItem>> {
    st.read(|c| tt::list_entries(c, &f)).map(Json)
}

pub async fn report(State(st): State<AppState>, Query(f): Query<TimeFilter>) -> ApiResult<TimeReport> {
    st.read(|c| tt::report(c, &f)).map(Json)
}

pub async fn create_entry(State(st): State<AppState>, Json(mut e): Json<TimeEntry>) -> ApiResult<TimeEntry> {
    e.id = 0;
    st.write(|c| tt::save_entry(c, e)).map(Json)
}

pub async fn update_entry(State(st): State<AppState>, Path(id): Path<i64>, Json(mut e): Json<TimeEntry>) -> ApiResult<TimeEntry> {
    e.id = id;
    st.write(|c| tt::save_entry(c, e)).map(Json)
}

pub async fn delete_entry(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<MessageResponse> {
    st.write(|c| tt::delete_entry(c, id)).map(|_| Json(MessageResponse { message: "Eintrag gelöscht".into() }))
}
