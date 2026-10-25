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

fn entries_in(conn: &Connection, f: &TimeFilter, docs: &DocumentIndex) -> Result<Vec<TimeEntry>> {
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
        // A project is its order together with the quote it came from.
        let ids = docs.project_of(id).iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
        sql.push_str(&format!(" AND document_id IN ({ids})"));
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
    /// quote | order (hours booked earlier may be on another type)
    pub document_type: String,
}

struct DocumentInfo {
    number: String,
    title: String,
    contact_name: String,
    doc_type: String,
}

/// The documents hours are booked on, and which of them belong to the same project: a quote
/// that was turned into an order shares the project with that order.
struct DocumentIndex {
    docs: HashMap<i64, DocumentInfo>,
    /// Document id -> the order its quote became (only for quotes that have one).
    order_of: HashMap<i64, i64>,
}

impl DocumentIndex {
    fn load(conn: &Connection) -> Result<Self> {
        let contacts: HashMap<i64, String> = db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c.display_name())).collect();
        let all = db::query::<Document>(conn, "ORDER BY id", &[])?;
        let mut order_of = HashMap::new();
        for d in all.iter().filter(|d| d.doc_type == "order") {
            if let Some(quote) = d.source_id {
                order_of.entry(quote).or_insert(d.id);
            }
        }
        let docs = all
            .into_iter()
            .map(|d| {
                let contact_name = d.contact_id.and_then(|id| contacts.get(&id).cloned()).unwrap_or_else(|| d.address.lines().next().unwrap_or_default().to_string());
                (d.id, DocumentInfo { number: d.number, title: d.title, contact_name, doc_type: d.doc_type })
            })
            .collect();
        Ok(Self { docs, order_of })
    }

    fn info(&self, id: i64) -> Option<&DocumentInfo> {
        self.docs.get(&id)
    }

    fn doc_type(&self, id: i64) -> &str {
        self.info(id).map_or("", |d| d.doc_type.as_str())
    }

    /// The document that stands for the project of `id`: the order a quote became, else itself.
    fn anchor(&self, id: i64) -> i64 {
        if self.doc_type(id) == "quote" { self.order_of.get(&id).copied().unwrap_or(id) } else { id }
    }

    /// All documents of the project `id` belongs to.
    fn project_of(&self, id: i64) -> Vec<i64> {
        let anchor = self.anchor(id);
        let mut ids: Vec<i64> = self.docs.keys().copied().filter(|d| self.anchor(*d) == anchor).collect();
        ids.sort_unstable();
        ids
    }
}

pub fn list_entries(conn: &Connection, f: &TimeFilter) -> Result<Vec<TimeEntryItem>> {
    let people: HashMap<i64, String> = list_people(conn)?.into_iter().map(|p| (p.id, p.name)).collect();
    let docs = DocumentIndex::load(conn)?;
    Ok(entries_in(conn, f, &docs)?
        .into_iter()
        .map(|entry| {
            let info = docs.info(entry.document_id);
            TimeEntryItem {
                person_name: people.get(&entry.person_id).cloned().unwrap_or_default(),
                document_number: info.map(|d| d.number.clone()).unwrap_or_default(),
                document_title: info.map(|d| d.title.clone()).unwrap_or_default(),
                contact_name: info.map(|d| d.contact_name.clone()).unwrap_or_default(),
                document_type: info.map(|d| d.doc_type.clone()).unwrap_or_default(),
                entry,
            }
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
    let Some(doc) = db::find::<Document>(conn, e.document_id)? else {
        bail_user!("Bitte ein Angebot oder eine Auftragsbestätigung wählen.");
    };
    // Hours booked earlier on another document type stay editable.
    let unchanged = e.id > 0 && db::get::<TimeEntry>(conn, e.id)?.document_id == e.document_id;
    if !["quote", "order"].contains(&doc.doc_type.as_str()) && !unchanged {
        bail_user!("Stunden lassen sich nur auf Angebote und Auftragsbestätigungen buchen.");
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
    /// Of which booked on a quote (preparing the offer).
    #[ts(type = "number")]
    pub quote_hours: Decimal,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DocumentHours {
    /// The order, or the quote while there is no order yet.
    pub document_id: i64,
    pub doc_type: String,
    pub number: String,
    pub title: String,
    pub contact_name: String,
    #[ts(type = "number")]
    pub total: Decimal,
    /// Of which booked on a quote (preparing the offer).
    #[ts(type = "number")]
    pub quote_hours: Decimal,
    pub people: Vec<PersonHours>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TimeReport {
    /// Hours per project (an order with the quote it came from), most hours first.
    pub documents: Vec<DocumentHours>,
    /// Hours per person over all documents of the filter, most hours first.
    pub people: Vec<PersonHours>,
    #[ts(type = "number")]
    pub total: Decimal,
    #[ts(type = "number")]
    pub quote_hours: Decimal,
}

fn by_hours(list: &mut [PersonHours]) {
    list.sort_by(|a, b| b.hours.cmp(&a.hours).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

pub fn report(conn: &Connection, f: &TimeFilter) -> Result<TimeReport> {
    let names: HashMap<i64, String> = list_people(conn)?.into_iter().map(|p| (p.id, p.name)).collect();
    let docs = DocumentIndex::load(conn)?;
    // project -> person -> (hours, of which on a quote)
    let mut per_doc: BTreeMap<i64, BTreeMap<i64, (Decimal, Decimal)>> = BTreeMap::new();
    let mut per_person: BTreeMap<i64, (Decimal, Decimal)> = BTreeMap::new();
    let (mut total, mut quote_total) = (Decimal::ZERO, Decimal::ZERO);
    for e in entries_in(conn, f, &docs)? {
        let quote = if docs.doc_type(e.document_id) == "quote" { e.hours } else { Decimal::ZERO };
        for slot in [per_doc.entry(docs.anchor(e.document_id)).or_default().entry(e.person_id).or_default(), per_person.entry(e.person_id).or_default()] {
            slot.0 += e.hours;
            slot.1 += quote;
        }
        total += e.hours;
        quote_total += quote;
    }
    let person_hours = |id: i64, (hours, quote_hours): (Decimal, Decimal)| PersonHours { person_id: id, name: names.get(&id).cloned().unwrap_or_default(), hours, quote_hours };
    let mut documents: Vec<DocumentHours> = per_doc
        .into_iter()
        .map(|(document_id, people)| {
            let mut people: Vec<PersonHours> = people.into_iter().map(|(id, h)| person_hours(id, h)).collect();
            by_hours(&mut people);
            let info = docs.info(document_id);
            DocumentHours {
                document_id,
                doc_type: info.map(|d| d.doc_type.clone()).unwrap_or_default(),
                number: info.map(|d| d.number.clone()).unwrap_or_default(),
                title: info.map(|d| d.title.clone()).unwrap_or_default(),
                contact_name: info.map(|d| d.contact_name.clone()).unwrap_or_default(),
                total: people.iter().map(|p| p.hours).sum(),
                quote_hours: people.iter().map(|p| p.quote_hours).sum(),
                people,
            }
        })
        .collect();
    documents.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.number.cmp(&b.number)));
    let mut people: Vec<PersonHours> = per_person.into_iter().map(|(id, h)| person_hours(id, h)).collect();
    by_hours(&mut people);
    Ok(TimeReport { documents, people, total, quote_hours: quote_total })
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

    #[test]
    fn a_quote_and_its_order_are_one_project_and_the_quote_hours_show() {
        let (conn, anna, ben, quote) = setup();
        conn.execute_batch(
            "INSERT INTO documents (id, doc_type, contact_id, date, number, title, source_id) VALUES (3, 'order', 1, '2026-01-20', 'AB-1', 'Website', 1);",
        )
        .unwrap();
        save_entry(&conn, entry(anna, quote, "2026-01-12", dec!(4))).unwrap();
        save_entry(&conn, entry(anna, 3, "2026-01-25", dec!(10))).unwrap();
        save_entry(&conn, entry(ben, 3, "2026-01-26", dec!(5))).unwrap();
        save_entry(&conn, entry(ben, 2, "2026-02-03", dec!(1))).unwrap();
        let r = report(&conn, &TimeFilter::default()).unwrap();
        assert_eq!(r.documents.len(), 2, "the quote is listed under its order");
        let project = &r.documents[0];
        assert_eq!((project.document_id, project.doc_type.as_str(), project.number.as_str()), (3, "order", "AB-1"));
        assert_eq!((project.total, project.quote_hours), (dec!(19), dec!(4)));
        let anna_hours = project.people.iter().find(|p| p.name == "Anna").unwrap();
        assert_eq!((anna_hours.hours, anna_hours.quote_hours), (dec!(14), dec!(4)));
        // A quote without an order is its own project, and all its hours are quote hours.
        let open_quote = &r.documents[1];
        assert_eq!((open_quote.document_id, open_quote.doc_type.as_str(), open_quote.total, open_quote.quote_hours), (2, "quote", dec!(1), dec!(1)));
        assert_eq!((r.total, r.quote_hours), (dec!(20), dec!(5)));
        // Filtering by the quote or by the order finds the whole project.
        for id in [1, 3] {
            let f = TimeFilter { document_id: Some(id), ..Default::default() };
            assert_eq!(list_entries(&conn, &f).unwrap().len(), 3, "document {id}");
            assert_eq!(report(&conn, &f).unwrap().total, dec!(19), "document {id}");
        }
    }

    #[test]
    fn hours_can_only_be_booked_on_quotes_and_orders() {
        let (conn, anna, _, _) = setup();
        conn.execute_batch("INSERT INTO documents (id, doc_type, date, number) VALUES (4, 'invoice', '2026-02-01', 'RE-1');").unwrap();
        assert!(save_entry(&conn, entry(anna, 4, "2026-02-02", dec!(1))).is_err());
        // Hours booked there earlier stay editable, but cannot be moved onto another invoice.
        conn.execute("INSERT INTO time_entries (id, person_id, document_id, date, hours) VALUES (50, ?1, 4, '2026-02-02', '2')", [anna]).unwrap();
        conn.execute_batch("INSERT INTO documents (id, doc_type, date, number) VALUES (5, 'invoice', '2026-02-01', 'RE-2');").unwrap();
        let edited = TimeEntry { id: 50, ..entry(anna, 4, "2026-02-02", dec!(3)) };
        assert_eq!(save_entry(&conn, edited).unwrap().hours, dec!(3));
        assert_eq!(list_entries(&conn, &TimeFilter::default()).unwrap()[0].document_type, "invoice");
        assert!(save_entry(&conn, TimeEntry { id: 50, ..entry(anna, 5, "2026-02-02", dec!(3)) }).is_err());
        assert!(save_entry(&conn, TimeEntry { id: 50, ..entry(anna, 1, "2026-02-02", dec!(3)) }).is_ok(), "moving onto a quote is fine");
    }
}
