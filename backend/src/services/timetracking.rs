// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Zeiterfassung: hours that people spent on a document (quote, order, …), and the evaluation
//! of who worked how long on what – the basis for sharing a profit fairly.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::{Contact, Document, Person, TimeEntry};
use crate::util::now_str;

pub fn list_people(conn: &Connection) -> Result<Vec<Person>> {
    db::query::<Person>(conn, "ORDER BY archived, name COLLATE NOCASE", &[])
}

pub fn save_person(conn: &Connection, mut p: Person) -> Result<Person> {
    p.name = p.name.trim().to_string();
    if p.name.is_empty() {
        bail_user!("Bitte einen Namen angeben.");
    }
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM people WHERE lower(name) = lower(?1) AND id != ?2", &[v(p.name.clone()), v(p.id)])? > 0 {
        bail_user!("Es gibt bereits eine Person mit diesem Namen.");
    }
    if p.id > 0 {
        let old: Person = db::get(conn, p.id)?;
        p.created_at = old.created_at;
        db::update(conn, &p)?;
    } else {
        p.created_at = now_str();
        db::insert(conn, &mut p)?;
    }
    Ok(p)
}

/// A person with recorded hours is archived instead, so the evaluation keeps their share.
pub fn delete_person(conn: &Connection, id: i64) -> Result<String> {
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM time_entries WHERE person_id = ?1", &[v(id)])? > 0 {
        let mut p: Person = db::get(conn, id)?;
        p.archived = true;
        db::update(conn, &p)?;
        return Ok("archived".into());
    }
    db::delete::<Person>(conn, id)?;
    Ok("deleted".into())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TimeFilter {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub person_id: Option<i64>,
    pub document_id: Option<i64>,
}

fn entries_of(conn: &Connection, f: &TimeFilter) -> Result<Vec<TimeEntry>> {
    let mut sql = String::from("WHERE 1 = 1");
    let mut params = Vec::new();
    if let Some(d) = f.from {
        params.push(v(d));
        sql.push_str(&format!(" AND date >= ?{}", params.len()));
    }
    if let Some(d) = f.to {
        params.push(v(d));
        sql.push_str(&format!(" AND date <= ?{}", params.len()));
    }
    if let Some(id) = f.person_id {
        params.push(v(id));
        sql.push_str(&format!(" AND person_id = ?{}", params.len()));
    }
    if let Some(id) = f.document_id {
        params.push(v(id));
        sql.push_str(&format!(" AND document_id = ?{}", params.len()));
    }
    sql.push_str(" ORDER BY date DESC, id DESC");
    db::query::<TimeEntry>(conn, &sql, &params)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TimeEntryItem {
    pub entry: TimeEntry,
    pub person_name: String,
    pub document_number: String,
    pub document_title: String,
    pub contact_name: String,
}

/// Names of the documents and their customers, by document id.
struct DocumentNames(HashMap<i64, (String, String, String)>);

impl DocumentNames {
    fn load(conn: &Connection) -> Result<Self> {
        let contacts: HashMap<i64, String> = db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c.display_name())).collect();
        let docs = db::query::<Document>(conn, "", &[])?;
        Ok(Self(
            docs.into_iter()
                .map(|d| {
                    let contact = d.contact_id.and_then(|id| contacts.get(&id).cloned()).unwrap_or_else(|| d.address.lines().next().unwrap_or_default().to_string());
                    (d.id, (d.number, d.title, contact))
                })
                .collect(),
        ))
    }

    fn get(&self, id: i64) -> (String, String, String) {
        self.0.get(&id).cloned().unwrap_or_default()
    }
}

pub fn list_entries(conn: &Connection, f: &TimeFilter) -> Result<Vec<TimeEntryItem>> {
    let people: HashMap<i64, String> = list_people(conn)?.into_iter().map(|p| (p.id, p.name)).collect();
    let docs = DocumentNames::load(conn)?;
    Ok(entries_of(conn, f)?
        .into_iter()
        .map(|entry| {
            let (document_number, document_title, contact_name) = docs.get(entry.document_id);
            TimeEntryItem { person_name: people.get(&entry.person_id).cloned().unwrap_or_default(), document_number, document_title, contact_name, entry }
        })
        .collect())
}

pub fn save_entry(conn: &Connection, mut e: TimeEntry) -> Result<TimeEntry> {
    e.hours = e.hours.round_dp(2);
    if e.hours <= Decimal::ZERO || e.hours > Decimal::from(24) {
        bail_user!("Die Stunden müssen größer als 0 und höchstens 24 sein.");
    }
    let person: Person = match db::find(conn, e.person_id)? {
        Some(p) => p,
        None => bail_user!("Bitte eine Person wählen."),
    };
    if db::find::<Document>(conn, e.document_id)?.is_none() {
        bail_user!("Bitte ein Angebot oder Dokument wählen.");
    }
    e.note = e.note.trim().to_string();
    e.updated_at = now_str();
    if e.id > 0 {
        let old: TimeEntry = db::get(conn, e.id)?;
        e.created_at = old.created_at;
        db::update(conn, &e)?;
    } else {
        if person.archived {
            bail_user!("{} ist archiviert. Für neue Einträge bitte eine aktive Person wählen.", person.name);
        }
        e.created_at = now_str();
        db::insert(conn, &mut e)?;
    }
    Ok(e)
}

pub fn delete_entry(conn: &Connection, id: i64) -> Result<()> {
    db::delete::<TimeEntry>(conn, id)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PersonHours {
    pub person_id: i64,
    pub name: String,
    #[ts(type = "number")]
    pub hours: Decimal,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DocumentHours {
    pub document_id: i64,
    pub number: String,
    pub title: String,
    pub contact_name: String,
    #[ts(type = "number")]
    pub total: Decimal,
    pub people: Vec<PersonHours>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TimeReport {
    /// Hours per document, most hours first.
    pub documents: Vec<DocumentHours>,
    /// Hours per person over all documents of the filter, most hours first.
    pub people: Vec<PersonHours>,
    #[ts(type = "number")]
    pub total: Decimal,
}

fn by_hours(list: &mut [PersonHours]) {
    list.sort_by(|a, b| b.hours.cmp(&a.hours).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

pub fn report(conn: &Connection, f: &TimeFilter) -> Result<TimeReport> {
    let names: HashMap<i64, String> = list_people(conn)?.into_iter().map(|p| (p.id, p.name)).collect();
    let docs = DocumentNames::load(conn)?;
    // document -> person -> hours
    let mut per_doc: BTreeMap<i64, BTreeMap<i64, Decimal>> = BTreeMap::new();
    let mut per_person: BTreeMap<i64, Decimal> = BTreeMap::new();
    let mut total = Decimal::ZERO;
    for e in entries_of(conn, f)? {
        *per_doc.entry(e.document_id).or_default().entry(e.person_id).or_default() += e.hours;
        *per_person.entry(e.person_id).or_default() += e.hours;
        total += e.hours;
    }
    let person_hours = |id: i64, hours: Decimal| PersonHours { person_id: id, name: names.get(&id).cloned().unwrap_or_default(), hours };
    let mut documents: Vec<DocumentHours> = per_doc
        .into_iter()
        .map(|(document_id, people)| {
            let mut people: Vec<PersonHours> = people.into_iter().map(|(id, h)| person_hours(id, h)).collect();
            by_hours(&mut people);
            let (number, title, contact_name) = docs.get(document_id);
            DocumentHours { document_id, number, title, contact_name, total: people.iter().map(|p| p.hours).sum(), people }
        })
        .collect();
    documents.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.number.cmp(&b.number)));
    let mut people: Vec<PersonHours> = per_person.into_iter().map(|(id, h)| person_hours(id, h)).collect();
    by_hours(&mut people);
    Ok(TimeReport { documents, people, total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn setup() -> (Connection, i64, i64, i64) {
        let conn = db::open_memory();
        conn.execute_batch(
            "INSERT INTO contacts (id, company) VALUES (1, 'Kunde GmbH');
             INSERT INTO documents (id, doc_type, contact_id, date, number, title) VALUES (1, 'quote', 1, '2026-01-10', 'AN-1', 'Website');
             INSERT INTO documents (id, doc_type, contact_id, date, number, title) VALUES (2, 'quote', 1, '2026-02-01', 'AN-2', 'App');",
        )
        .unwrap();
        let anna = save_person(&conn, Person { name: "Anna".into(), ..Default::default() }).unwrap().id;
        let ben = save_person(&conn, Person { name: "Ben".into(), ..Default::default() }).unwrap().id;
        (conn, anna, ben, 1)
    }

    fn entry(person_id: i64, document_id: i64, date: &str, hours: Decimal) -> TimeEntry {
        TimeEntry { person_id, document_id, date: date.parse().unwrap(), hours, ..Default::default() }
    }

    #[test]
    fn the_report_adds_up_hours_per_person_and_document() {
        let (conn, anna, ben, doc) = setup();
        save_entry(&conn, entry(anna, doc, "2026-01-12", dec!(6))).unwrap();
        save_entry(&conn, entry(anna, doc, "2026-01-13", dec!(2.5))).unwrap();
        save_entry(&conn, entry(ben, doc, "2026-01-13", dec!(3))).unwrap();
        save_entry(&conn, entry(ben, 2, "2026-02-03", dec!(4))).unwrap();
        let r = report(&conn, &TimeFilter::default()).unwrap();
        assert_eq!(r.total, dec!(15.5));
        assert_eq!(r.people.iter().map(|p| (p.name.as_str(), p.hours)).collect::<Vec<_>>(), [("Anna", dec!(8.5)), ("Ben", dec!(7))]);
        assert_eq!(r.documents[0].number, "AN-1");
        assert_eq!(r.documents[0].contact_name, "Kunde GmbH");
        assert_eq!(r.documents[0].total, dec!(11.5));
        assert_eq!(r.documents[0].people.len(), 2);
        assert_eq!(r.documents[1].total, dec!(4));
    }

    #[test]
    fn the_filter_limits_the_period_person_and_document() {
        let (conn, anna, ben, doc) = setup();
        save_entry(&conn, entry(anna, doc, "2026-01-12", dec!(6))).unwrap();
        save_entry(&conn, entry(ben, doc, "2026-02-03", dec!(4))).unwrap();
        save_entry(&conn, entry(ben, 2, "2026-02-04", dec!(1))).unwrap();
        let february = TimeFilter { from: Some("2026-02-01".parse().unwrap()), to: Some("2026-02-28".parse().unwrap()), ..Default::default() };
        assert_eq!(report(&conn, &february).unwrap().total, dec!(5));
        assert_eq!(report(&conn, &TimeFilter { person_id: Some(anna), ..Default::default() }).unwrap().total, dec!(6));
        assert_eq!(list_entries(&conn, &TimeFilter { document_id: Some(2), ..Default::default() }).unwrap().len(), 1);
        let items = list_entries(&conn, &TimeFilter::default()).unwrap();
        assert_eq!(items[0].entry.date.to_string(), "2026-02-04", "newest first");
        assert_eq!(items[0].person_name, "Ben");
        assert_eq!(items[0].document_title, "App");
    }

    #[test]
    fn hours_and_references_are_validated() {
        let (conn, anna, _, doc) = setup();
        assert!(save_entry(&conn, entry(anna, doc, "2026-01-12", dec!(0))).is_err());
        assert!(save_entry(&conn, entry(anna, doc, "2026-01-12", dec!(24.5))).is_err());
        assert!(save_entry(&conn, entry(999, doc, "2026-01-12", dec!(1))).is_err());
        assert!(save_entry(&conn, entry(anna, 999, "2026-01-12", dec!(1))).is_err());
        assert_eq!(save_entry(&conn, entry(anna, doc, "2026-01-12", dec!(1.256))).unwrap().hours, dec!(1.26));
    }

    #[test]
    fn people_are_unique_and_archived_when_they_have_hours() {
        let (conn, anna, ben, doc) = setup();
        assert!(save_person(&conn, Person { name: " anna ".into(), ..Default::default() }).is_err());
        assert!(save_person(&conn, Person { name: "  ".into(), ..Default::default() }).is_err());
        save_entry(&conn, entry(anna, doc, "2026-01-12", dec!(2))).unwrap();
        assert_eq!(delete_person(&conn, anna).unwrap(), "archived");
        assert_eq!(delete_person(&conn, ben).unwrap(), "deleted");
        let people = list_people(&conn).unwrap();
        assert_eq!(people.len(), 1);
        assert!(people[0].archived);
        // Archived: no new hours, but the old ones stay in the report.
        assert!(save_entry(&conn, entry(anna, doc, "2026-01-13", dec!(1))).is_err());
        assert_eq!(report(&conn, &TimeFilter::default()).unwrap().total, dec!(2));
    }

    #[test]
    fn a_document_with_hours_cannot_be_deleted() {
        let (conn, anna, _, doc) = setup();
        let e = save_entry(&conn, entry(anna, doc, "2026-01-12", dec!(2))).unwrap();
        let err = crate::services::documents::delete(&conn, doc).unwrap_err().to_string();
        assert!(err.contains("Stunden erfasst"), "{err}");
        delete_entry(&conn, e.id).unwrap();
        crate::services::documents::delete(&conn, doc).unwrap();
    }
}
