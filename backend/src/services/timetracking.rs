// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Projektstunden: hours that people spent on a quote or order, and the evaluation of who worked
//! how long on which project.
//!
//! A project is a quote together with everything that was made of it (the orders converted from
//! it); an order without a quote is a project of its own. Hours are booked on quotes and orders
//! only – before an order exists the quote is the only document.

use std::collections::{BTreeMap, BTreeSet, HashMap};

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
    /// A quote or an order: the entries of its whole project.
    pub document_id: Option<i64>,
}

/// The quote an order was converted from, if it was; the document itself otherwise.
fn root_of(conn: &Connection, id: i64) -> Result<i64> {
    let row: Option<(String, Option<i64>)> = conn.query_row("SELECT doc_type, source_id FROM documents WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).ok();
    if let Some((doc_type, Some(source))) = row
        && doc_type == "order"
        && db::scalar_i64(conn, "SELECT COUNT(*) FROM documents WHERE id = ?1 AND doc_type = 'quote'", &[v(source)])? > 0
    {
        return Ok(source);
    }
    Ok(id)
}

/// The project of `id`: its root and every order converted from the root.
fn project_ids(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let root = root_of(conn, id)?;
    let mut stmt = conn.prepare("SELECT id FROM documents WHERE id = ?1 OR (doc_type = 'order' AND source_id = ?1) ORDER BY id")?;
    let ids = stmt.query_map([root], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

fn id_list(ids: impl IntoIterator<Item = i64>) -> String {
    ids.into_iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
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
        sql.push_str(&format!(" AND document_id IN ({})", id_list(project_ids(conn, id)?)));
    }
    sql.push_str(" ORDER BY date DESC, id DESC");
    db::query::<TimeEntry>(conn, &sql, &params)
}

struct DocumentInfo {
    doc_type: String,
    status: String,
    number: String,
    title: String,
    contact_name: String,
    source_id: Option<i64>,
}

/// What the hours refer to: the documents they are booked on and the quotes of those that are
/// orders. Only these are loaded, not the whole document table.
struct DocumentIndex {
    docs: HashMap<i64, DocumentInfo>,
    /// Quote id -> numbers of the orders converted from it.
    orders: HashMap<i64, Vec<String>>,
}

impl DocumentIndex {
    fn load(conn: &Connection, ids: &BTreeSet<i64>) -> Result<Self> {
        let mut docs = Self::fetch(conn, ids)?;
        // The quotes the booked orders came from.
        let quotes: BTreeSet<i64> = docs.values().filter(|d| d.doc_type == "order").filter_map(|d| d.source_id).filter(|q| !docs.contains_key(q)).collect();
        docs.extend(Self::fetch(conn, &quotes)?);
        let mut index = Self { docs, orders: HashMap::new() };
        let roots: BTreeSet<i64> = index.docs.keys().map(|id| index.root(*id)).filter(|r| index.doc_type(*r) == "quote").collect();
        if !roots.is_empty() {
            let sql = format!("SELECT source_id, number FROM documents WHERE doc_type = 'order' AND number != '' AND source_id IN ({}) ORDER BY number", id_list(roots));
            let mut stmt = conn.prepare(&sql)?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
                let (quote, number) = row?;
                index.orders.entry(quote).or_default().push(number);
            }
        }
        Ok(index)
    }

    fn fetch(conn: &Connection, ids: &BTreeSet<i64>) -> Result<HashMap<i64, DocumentInfo>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let list = id_list(ids.iter().copied());
        let docs = db::query::<Document>(conn, &format!("WHERE id IN ({list})"), &[])?;
        let contact_ids: BTreeSet<i64> = docs.iter().filter_map(|d| d.contact_id).collect();
        let contacts: HashMap<i64, String> = if contact_ids.is_empty() {
            HashMap::new()
        } else {
            db::query::<Contact>(conn, &format!("WHERE id IN ({})", id_list(contact_ids)), &[])?.into_iter().map(|c| (c.id, c.display_name())).collect()
        };
        Ok(docs
            .into_iter()
            .map(|d| {
                let contact_name = d.contact_id.and_then(|id| contacts.get(&id).cloned()).unwrap_or_else(|| d.address.lines().next().unwrap_or_default().to_string());
                (d.id, DocumentInfo { doc_type: d.doc_type, status: d.status, number: d.number, title: d.title, contact_name, source_id: d.source_id })
            })
            .collect())
    }

    fn doc_type(&self, id: i64) -> &str {
        self.docs.get(&id).map_or("", |d| d.doc_type.as_str())
    }

    /// The project a document belongs to: the quote an order came from, else the document itself.
    fn root(&self, id: i64) -> i64 {
        match self.docs.get(&id) {
            Some(d) if d.doc_type == "order" => d.source_id.filter(|q| self.doc_type(*q) == "quote").unwrap_or(id),
            _ => id,
        }
    }
}

fn document_ids(entries: &[TimeEntry]) -> BTreeSet<i64> {
    entries.iter().map(|e| e.document_id).collect()
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

pub fn list_entries(conn: &Connection, f: &TimeFilter) -> Result<Vec<TimeEntryItem>> {
    let people: HashMap<i64, String> = list_people(conn)?.into_iter().map(|p| (p.id, p.name)).collect();
    let entries = entries_of(conn, f)?;
    let docs = DocumentIndex::load(conn, &document_ids(&entries))?;
    Ok(entries
        .into_iter()
        .map(|entry| {
            let info = docs.docs.get(&entry.document_id);
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
    /// Of which worked while preparing the offer.
    #[ts(type = "number")]
    pub offer_hours: Decimal,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProjectHours {
    /// The quote, or the order if it has no quote.
    pub document_id: i64,
    /// quote | order
    pub doc_type: String,
    /// Status of the root document, e.g. open (sent), accepted, rejected.
    pub status: String,
    pub number: String,
    pub title: String,
    pub contact_name: String,
    /// Numbers of the orders converted from the quote.
    pub orders: Vec<String>,
    #[ts(type = "number")]
    pub total: Decimal,
    /// Of which worked while preparing the offer.
    #[ts(type = "number")]
    pub offer_hours: Decimal,
    pub people: Vec<PersonHours>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TimeReport {
    /// Hours per project, most hours first.
    pub projects: Vec<ProjectHours>,
    /// Hours per person over all projects of the filter, most hours first.
    pub people: Vec<PersonHours>,
    #[ts(type = "number")]
    pub total: Decimal,
    #[ts(type = "number")]
    pub offer_hours: Decimal,
}

fn by_hours(list: &mut [PersonHours]) {
    list.sort_by(|a, b| b.hours.cmp(&a.hours).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

/// (hours, of which in the offer phase)
type Hours = (Decimal, Decimal);

pub fn report(conn: &Connection, f: &TimeFilter) -> Result<TimeReport> {
    let names: HashMap<i64, String> = list_people(conn)?.into_iter().map(|p| (p.id, p.name)).collect();
    let entries = entries_of(conn, f)?;
    let docs = DocumentIndex::load(conn, &document_ids(&entries))?;
    let mut per_project: BTreeMap<i64, BTreeMap<i64, Hours>> = BTreeMap::new();
    let mut per_person: BTreeMap<i64, Hours> = BTreeMap::new();
    let mut sum: Hours = Default::default();
    for e in &entries {
        let offer = if e.offer_phase { e.hours } else { Decimal::ZERO };
        for slot in [per_project.entry(docs.root(e.document_id)).or_default().entry(e.person_id).or_default(), per_person.entry(e.person_id).or_default(), &mut sum] {
            slot.0 += e.hours;
            slot.1 += offer;
        }
    }
    let person_hours = |id: i64, (hours, offer_hours): Hours| PersonHours { person_id: id, name: names.get(&id).cloned().unwrap_or_default(), hours, offer_hours };
    let mut projects: Vec<ProjectHours> = per_project
        .into_iter()
        .map(|(document_id, people)| {
            let mut people: Vec<PersonHours> = people.into_iter().map(|(id, h)| person_hours(id, h)).collect();
            by_hours(&mut people);
            let info = docs.docs.get(&document_id);
            ProjectHours {
                document_id,
                doc_type: info.map(|d| d.doc_type.clone()).unwrap_or_default(),
                status: info.map(|d| d.status.clone()).unwrap_or_default(),
                number: info.map(|d| d.number.clone()).unwrap_or_default(),
                title: info.map(|d| d.title.clone()).unwrap_or_default(),
                contact_name: info.map(|d| d.contact_name.clone()).unwrap_or_default(),
                orders: docs.orders.get(&document_id).cloned().unwrap_or_default(),
                total: people.iter().map(|p| p.hours).sum(),
                offer_hours: people.iter().map(|p| p.offer_hours).sum(),
                people,
            }
        })
        .collect();
    projects.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.number.cmp(&b.number)));
    let mut people: Vec<PersonHours> = per_person.into_iter().map(|(id, h)| person_hours(id, h)).collect();
    by_hours(&mut people);
    Ok(TimeReport { projects, people, total: sum.0, offer_hours: sum.1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn setup() -> (Connection, i64, i64, i64) {
        let conn = db::open_memory();
        conn.execute_batch(
            "INSERT INTO contacts (id, company) VALUES (1, 'Kunde GmbH');
             INSERT INTO documents (id, doc_type, contact_id, date, number, title, status) VALUES (1, 'quote', 1, '2026-01-10', 'AN-1', 'Website', 'accepted');
             INSERT INTO documents (id, doc_type, contact_id, date, number, title, status) VALUES (2, 'quote', 1, '2026-02-01', 'AN-2', 'App', 'open');",
        )
        .unwrap();
        let anna = save_person(&conn, Person { name: "Anna".into(), ..Default::default() }).unwrap().id;
        let ben = save_person(&conn, Person { name: "Ben".into(), ..Default::default() }).unwrap().id;
        (conn, anna, ben, 1)
    }

    fn entry(person_id: i64, document_id: i64, date: &str, hours: Decimal) -> TimeEntry {
        TimeEntry { person_id, document_id, date: date.parse().unwrap(), hours, ..Default::default() }
    }

    fn offer(person_id: i64, document_id: i64, date: &str, hours: Decimal) -> TimeEntry {
        TimeEntry { offer_phase: true, ..entry(person_id, document_id, date, hours) }
    }

    #[test]
    fn the_report_adds_up_hours_per_person_and_project() {
        let (conn, anna, ben, doc) = setup();
        save_entry(&conn, entry(anna, doc, "2026-01-12", dec!(6))).unwrap();
        save_entry(&conn, entry(anna, doc, "2026-01-13", dec!(2.5))).unwrap();
        save_entry(&conn, entry(ben, doc, "2026-01-13", dec!(3))).unwrap();
        save_entry(&conn, entry(ben, 2, "2026-02-03", dec!(4))).unwrap();
        let r = report(&conn, &TimeFilter::default()).unwrap();
        assert_eq!(r.total, dec!(15.5));
        assert_eq!(r.offer_hours, dec!(0));
        assert_eq!(r.people.iter().map(|p| (p.name.as_str(), p.hours)).collect::<Vec<_>>(), [("Anna", dec!(8.5)), ("Ben", dec!(7))]);
        assert_eq!(r.projects[0].number, "AN-1");
        assert_eq!(r.projects[0].contact_name, "Kunde GmbH");
        assert_eq!(r.projects[0].status, "accepted");
        assert_eq!(r.projects[0].total, dec!(11.5));
        assert_eq!(r.projects[0].people.len(), 2);
        assert_eq!(r.projects[1].total, dec!(4));
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
    fn a_quote_and_all_its_orders_are_one_project_with_the_offer_hours_shown() {
        let (conn, anna, ben, quote) = setup();
        conn.execute_batch(
            "INSERT INTO documents (id, doc_type, contact_id, date, number, title, source_id, status) VALUES (3, 'order', 1, '2026-01-20', 'AB-1', 'Website', 1, 'open');
             INSERT INTO documents (id, doc_type, contact_id, date, number, title, source_id, status) VALUES (4, 'order', 1, '2026-01-22', 'AB-2', 'Website 2', 1, 'open');
             INSERT INTO documents (id, doc_type, contact_id, date, number, title, status) VALUES (5, 'order', 1, '2026-03-01', 'AB-3', 'Ohne Angebot', 'open');",
        )
        .unwrap();
        save_entry(&conn, offer(anna, quote, "2026-01-12", dec!(4))).unwrap();
        save_entry(&conn, entry(anna, 3, "2026-01-25", dec!(10))).unwrap();
        save_entry(&conn, entry(ben, 3, "2026-01-26", dec!(5))).unwrap();
        save_entry(&conn, entry(ben, 4, "2026-01-27", dec!(2))).unwrap();
        save_entry(&conn, entry(ben, 5, "2026-03-02", dec!(1))).unwrap();
        let r = report(&conn, &TimeFilter::default()).unwrap();
        assert_eq!(r.projects.len(), 2, "both orders and the quote are listed under the quote");
        let project = &r.projects[0];
        assert_eq!((project.document_id, project.doc_type.as_str(), project.number.as_str()), (1, "quote", "AN-1"));
        assert_eq!(project.orders, ["AB-1", "AB-2"], "every order converted from the quote, not only the first");
        assert_eq!((project.total, project.offer_hours), (dec!(21), dec!(4)));
        let anna_hours = project.people.iter().find(|p| p.name == "Anna").unwrap();
        assert_eq!((anna_hours.hours, anna_hours.offer_hours), (dec!(14), dec!(4)));
        // An order without a quote is its own project.
        let own = &r.projects[1];
        assert_eq!((own.document_id, own.doc_type.as_str(), own.total, own.offer_hours), (5, "order", dec!(1), dec!(0)));
        assert!(own.orders.is_empty());
        assert_eq!((r.total, r.offer_hours), (dec!(22), dec!(4)));
        // Filtering by the quote or by any of its orders finds the whole project.
        for id in [1, 3, 4] {
            let f = TimeFilter { document_id: Some(id), ..Default::default() };
            assert_eq!(list_entries(&conn, &f).unwrap().len(), 4, "document {id}");
            assert_eq!(report(&conn, &f).unwrap().total, dec!(21), "document {id}");
        }
    }

    #[test]
    fn a_quote_that_was_invoiced_without_an_order_is_an_accepted_project() {
        let (conn, anna, _, quote) = setup();
        save_entry(&conn, offer(anna, quote, "2026-01-12", dec!(2))).unwrap();
        save_entry(&conn, entry(anna, quote, "2026-01-20", dec!(8))).unwrap();
        let r = report(&conn, &TimeFilter::default()).unwrap();
        let project = &r.projects[0];
        assert_eq!((project.status.as_str(), project.orders.len()), ("accepted", 0));
        assert_eq!((project.total, project.offer_hours), (dec!(10), dec!(2)), "only what was marked counts as the offer phase");
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
