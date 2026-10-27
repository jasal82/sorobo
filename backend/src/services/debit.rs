// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! SEPA-Lastschriften: mandates of customers, invoices with the payment method "debit" (the
//! invoice is the pre-notification), pain.008.001.08 files for the upload in online banking, and
//! the bank side – collections booked as one batch and Rücklastschriften (return debits) that
//! reopen the invoice.
//!
//! Whether a collection came back is not stored but derived: every negative bank payment assigned
//! to the invoice marks one of its collections (the oldest first) as returned.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::{Duration, Local, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::{Allocation, Contact, DebitRun, DebitRunItem, Document, Mandate, PaymentAccount, Settings, Transaction};
use crate::services::finance::{self, MatchSuggestion, TransactionInput};
use crate::services::sepa::{EEA, NON_EEA_SEPA, amount, country, sepa_id, sepa_text, xml_escape};
use crate::services::settings as cfg;
use crate::util::{add_months, bic_valid, creditor_id_valid, fmt_date, fmt_money, iban_valid, mask_iban, normalize_iban, now_str, target_day_from, today};

/// A mandate expires when it was not used for 36 months (EPC rulebook).
const EXPIRY_MONTHS: i32 = 36;

/// Earliest collection date for a file handed to the bank today: the next TARGET2 business day
/// (D-1 for CORE and B2B).
pub fn earliest_collection(today: NaiveDate) -> NaiveDate {
    target_day_from(today + Duration::days(1))
}

// ---------------------------------------------------------------------------
// Collections and their state
// ---------------------------------------------------------------------------

struct ItemState {
    item: DebitRunItem,
    returned: bool,
}

/// All collections with their state, oldest first.
fn item_states(conn: &Connection) -> Result<Vec<ItemState>> {
    let items = db::query::<DebitRunItem>(conn, "ORDER BY collection_date, id", &[])?;
    let mut returns: HashMap<i64, usize> = HashMap::new();
    let mut stmt = conn.prepare_cached("SELECT document_id, COUNT(*) FROM allocations WHERE document_id IS NOT NULL AND transaction_id IS NOT NULL AND CAST(amount AS REAL) < 0 GROUP BY document_id")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (doc, n) = row?;
        returns.insert(doc, n as usize);
    }
    let mut seen: HashMap<i64, usize> = HashMap::new();
    Ok(items
        .into_iter()
        .map(|item| {
            let k = seen.entry(item.document_id).or_insert(0);
            *k += 1;
            let returned = *k <= returns.get(&item.document_id).copied().unwrap_or(0);
            ItemState { item, returned }
        })
        .collect())
}

#[derive(Default, Clone, Copy)]
struct Usage {
    collections: i64,
    last: Option<NaiveDate>,
}

fn usage(states: &[ItemState]) -> HashMap<i64, Usage> {
    let mut out: HashMap<i64, Usage> = HashMap::new();
    for s in states.iter().filter(|s| !s.returned) {
        let u = out.entry(s.item.mandate_id).or_default();
        u.collections += 1;
        u.last = u.last.max(Some(s.item.collection_date));
    }
    out
}

/// Latest collection of each invoice that did not come back (the invoice waits for the money).
fn pending(states: &[ItemState]) -> HashMap<i64, &DebitRunItem> {
    let mut out = HashMap::new();
    let mut returned: HashSet<i64> = HashSet::new();
    for s in states {
        if s.returned {
            returned.insert(s.item.document_id);
            out.remove(&s.item.document_id);
        } else {
            returned.remove(&s.item.document_id);
            out.insert(s.item.document_id, &s.item);
        }
    }
    out
}

/// Invoices whose last collection came back.
fn returned_docs(states: &[ItemState]) -> HashSet<i64> {
    let mut out = HashSet::new();
    for s in states {
        if s.returned {
            out.insert(s.item.document_id);
        } else {
            out.remove(&s.item.document_id);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Mandates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct MandateView {
    pub mandate: Mandate,
    /// active | revoked | expired | used
    pub status: String,
    pub status_label: String,
    pub usable: bool,
    pub collections: i64,
    pub last_collection: Option<NaiveDate>,
    /// End of validity if it is not used again.
    pub expires_on: Option<NaiveDate>,
}

fn view(m: Mandate, u: Usage, today: NaiveDate) -> MandateView {
    let expires_on = add_months(u.last.unwrap_or(m.signed_on), EXPIRY_MONTHS);
    let (status, label) = if let Some(r) = m.revoked_on.filter(|r| *r <= today) {
        ("revoked", format!("widerrufen am {}", fmt_date(r)))
    } else if !m.recurring && u.collections > 0 {
        ("used", "Einmal-Mandat bereits genutzt".to_string())
    } else if today > expires_on {
        ("expired", format!("verfallen am {} (36 Monate nicht genutzt)", fmt_date(expires_on)))
    } else {
        ("active", "aktiv".to_string())
    };
    MandateView {
        status: status.into(),
        status_label: label,
        usable: status == "active",
        collections: u.collections,
        last_collection: u.last,
        expires_on: (status == "active").then_some(expires_on),
        mandate: m,
    }
}

pub fn mandates(conn: &Connection, contact_id: Option<i64>) -> Result<Vec<MandateView>> {
    let usage = usage(&item_states(conn)?);
    let list = match contact_id {
        Some(c) => db::query::<Mandate>(conn, "WHERE contact_id = ?1 ORDER BY signed_on DESC, id DESC", &[v(c)])?,
        None => db::query::<Mandate>(conn, "ORDER BY signed_on DESC, id DESC", &[])?,
    };
    let t = today();
    Ok(list.into_iter().map(|m| {
        let u = usage.get(&m.id).copied().unwrap_or_default();
        view(m, u, t)
    }).collect())
}

fn mandate_view(conn: &Connection, id: i64) -> Result<Option<MandateView>> {
    let Some(m) = db::find::<Mandate>(conn, id)? else { return Ok(None) };
    let u = usage(&item_states(conn)?).get(&id).copied().unwrap_or_default();
    Ok(Some(view(m, u, today())))
}

/// The mandate new invoices of a customer are collected with: the latest usable one.
pub fn usable_mandate(conn: &Connection, contact_id: i64) -> Result<Option<Mandate>> {
    Ok(mandates(conn, Some(contact_id))?.into_iter().find(|m| m.usable).map(|m| m.mandate))
}

/// Suggested Mandatsreferenz: customer number and a running number.
pub fn suggest_reference(conn: &Connection, contact_id: i64) -> Result<String> {
    let c: Contact = db::get(conn, contact_id)?;
    let base = sepa_id(&if c.number.trim().is_empty() { format!("K{}", c.id) } else { c.number.clone() });
    let mut n = db::scalar_i64(conn, "SELECT COUNT(*) FROM mandates WHERE contact_id = ?1", &[v(contact_id)])? + 1;
    loop {
        let r = format!("{base}-{n}");
        if db::scalar_i64(conn, "SELECT COUNT(*) FROM mandates WHERE reference = ?1", &[v(r.clone())])? == 0 {
            return Ok(r);
        }
        n += 1;
    }
}

/// Mandatsreferenz: letters, digits and + ? / - : ( ) . , ' without spaces, at most 35.
fn reference_valid(r: &str) -> bool {
    !r.is_empty() && r.len() <= 35 && r.chars().all(|c| c.is_ascii_alphanumeric() || "+?/-:().,'".contains(c))
}

pub fn save_mandate(conn: &Connection, mut m: Mandate) -> Result<MandateView> {
    let contact: Contact = match db::find(conn, m.contact_id)? {
        Some(c) => c,
        None => bail_user!("Kontakt nicht gefunden."),
    };
    m.reference = m.reference.trim().to_string();
    m.iban = normalize_iban(&m.iban);
    m.bic = normalize_iban(&m.bic);
    m.account_holder = m.account_holder.trim().to_string();
    if m.account_holder.is_empty() {
        m.account_holder = contact.display_name();
    }
    if m.scheme != "B2B" {
        m.scheme = "CORE".into();
    }
    if !reference_valid(&m.reference) {
        bail_user!("Die Mandatsreferenz darf höchstens 35 Zeichen lang sein und nur Buchstaben (ohne Umlaute), Ziffern und + ? / - : ( ) . , ' enthalten.");
    }
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM mandates WHERE reference = ?1 AND id != ?2", &[v(m.reference.clone()), v(m.id)])? > 0 {
        bail_user!("Die Mandatsreferenz {} ist bereits vergeben.", m.reference);
    }
    if !iban_valid(&m.iban) {
        bail_user!("Bitte eine gültige IBAN angeben.");
    }
    let cc = country(&m.iban);
    if !EEA.contains(&cc) && !NON_EEA_SEPA.contains(&cc) {
        bail_user!("Die IBAN aus {cc} liegt nicht im SEPA-Raum.");
    }
    if !m.bic.is_empty() && !bic_valid(&m.bic) {
        bail_user!("Die BIC ist ungültig (8 oder 11 Zeichen).");
    }
    if m.signed_on > today() {
        bail_user!("Das Mandat kann nicht in der Zukunft unterschrieben sein.");
    }
    if m.revoked_on.is_some_and(|r| r < m.signed_on) {
        bail_user!("Der Widerruf liegt vor der Unterschrift.");
    }
    if m.id > 0 {
        let old: Mandate = db::get(conn, m.id)?;
        let used = db::scalar_i64(conn, "SELECT COUNT(*) FROM debit_run_items WHERE mandate_id = ?1", &[v(m.id)])? > 0;
        if used && (old.reference != m.reference || old.iban != m.iban || old.signed_on != m.signed_on || old.scheme != m.scheme || old.contact_id != m.contact_id) {
            bail_user!("Mit diesem Mandat wurde bereits eingezogen. Referenz, IBAN, Datum und Art können nicht mehr geändert werden – bei einem Kontowechsel bitte ein neues Mandat anlegen.");
        }
        m.created_at = old.created_at;
        db::update(conn, &m)?;
        if old.revoked_on.is_none() && m.revoked_on.is_some() {
            cfg::log(conn, "contact", m.contact_id, "mandate", format!("Lastschriftmandat {} widerrufen zum {}", m.reference, fmt_date(m.revoked_on.unwrap_or_default())))?;
        }
    } else {
        m.created_at = now_str();
        db::insert(conn, &mut m)?;
        cfg::log(conn, "contact", m.contact_id, "mandate", format!("Lastschriftmandat {} vom {} erfasst", m.reference, fmt_date(m.signed_on)))?;
    }
    Ok(mandate_view(conn, m.id)?.expect("saved mandate"))
}

pub fn delete_mandate(conn: &Connection, id: i64) -> Result<()> {
    let m: Mandate = db::get(conn, id)?;
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM debit_run_items WHERE mandate_id = ?1", &[v(id)])? > 0 {
        bail_user!("Mit diesem Mandat wurde bereits eingezogen. Es muss aufbewahrt werden – bitte stattdessen widerrufen.");
    }
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM documents WHERE mandate_id = ?1 AND status != 'draft'", &[v(id)])? > 0 {
        bail_user!("Das Mandat ist in abgeschlossenen Rechnungen angegeben – bitte stattdessen widerrufen.");
    }
    conn.execute("UPDATE documents SET mandate_id = NULL, payment_method = '' WHERE mandate_id = ?1 AND status = 'draft'", [id])?;
    db::delete::<Mandate>(conn, id)?;
    cfg::log(conn, "contact", m.contact_id, "mandate", format!("Lastschriftmandat {} gelöscht", m.reference))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Invoices collected by direct debit
// ---------------------------------------------------------------------------

/// Payment method and mandate of a new invoice for a customer with a usable mandate; the closing
/// text follows the payment method.
pub fn apply_default(conn: &Connection, s: &Settings, doc: &mut Document) -> Result<()> {
    if doc.doc_type != "invoice" || doc.kind == "cancellation" {
        return Ok(());
    }
    let mandate = match doc.contact_id {
        Some(c) => usable_mandate(conn, c)?,
        None => None,
    };
    let method = if mandate.is_some() { "debit" } else { "" };
    crate::services::documents::swap_closing(s, doc, method);
    doc.payment_method = method.into();
    doc.mandate_id = mandate.map(|m| m.id);
    Ok(())
}

/// Why an invoice cannot be collected (empty = ok).
pub fn document_problems(conn: &Connection, s: &Settings, doc: &Document) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if !creditor_id_valid(&s.debit.creditor_id) {
        out.push(if s.debit.creditor_id.trim().is_empty() {
            "Gläubiger-ID fehlt (Einstellungen → Unternehmen)".to_string()
        } else {
            "Gläubiger-ID ist ungültig (Einstellungen → Unternehmen)".to_string()
        });
    }
    let Some(m) = doc.mandate_id.map(|id| mandate_view(conn, id)).transpose()?.flatten() else {
        out.push("Kein Lastschriftmandat ausgewählt".into());
        return Ok(out);
    };
    if Some(m.mandate.contact_id) != doc.contact_id {
        out.push(format!("Das Mandat {} gehört zu einem anderen Kunden", m.mandate.reference));
    }
    // The invoice's own earlier collection counts for a one-off mandate.
    let own = db::scalar_i64(conn, "SELECT COUNT(*) FROM debit_run_items WHERE document_id = ?1 AND mandate_id = ?2", &[v(doc.id), v(m.mandate.id)])? > 0;
    if !m.usable && !(m.status == "used" && own) {
        out.push(format!("Mandat {}: {}", m.mandate.reference, m.status_label));
    }
    let cc = country(&m.mandate.iban);
    if NON_EEA_SEPA.contains(&cc) {
        let contact = doc.contact_id.map(|c| db::find::<Contact>(conn, c)).transpose()?.flatten().unwrap_or_default();
        if contact.city.trim().is_empty() || contact.country.trim().len() != 2 {
            out.push("Anschrift des Kunden (Ort, Land) fehlt – für Konten außerhalb des EWR erforderlich".into());
        }
    }
    Ok(out)
}

/// Checks before an invoice paid by direct debit is issued; also the pre-notification period.
pub fn check_issue(conn: &Connection, s: &Settings, doc: &Document) -> Result<()> {
    let problems = document_problems(conn, s, doc)?;
    if !problems.is_empty() {
        bail_user!("SEPA-Lastschrift nicht möglich: {}.", problems.join("; "));
    }
    let days = s.debit.prenotification_days.max(1);
    if doc.due_date.is_none_or(|d| d < doc.date + Duration::days(days)) {
        bail_user!(
            "Die Rechnung kündigt die Lastschrift an und muss dem Kunden mindestens {days} Tage vor dem Einzug vorliegen. Bitte ein Zahlungsziel von mindestens {days} Tagen wählen (oder die Frist der Vorabankündigung in den Einstellungen anpassen, wenn mit den Kunden eine kürzere Frist vereinbart ist)."
        );
    }
    Ok(())
}

/// Placeholders of the pre-notification text: Mandatsreferenz, Gläubiger-ID, Kunden-IBAN.
pub fn placeholders(conn: &Connection, s: &Settings, doc: &Document) -> Result<Vec<(&'static str, String)>> {
    let m = match doc.mandate_id.filter(|_| doc.payment_method == "debit") {
        Some(id) => db::find::<Mandate>(conn, id)?,
        None => None,
    };
    Ok(vec![
        ("Mandatsreferenz", m.as_ref().map(|m| m.reference.clone()).unwrap_or_default()),
        ("Gläubiger-ID", if m.is_some() { normalize_iban(&s.debit.creditor_id) } else { String::new() }),
        ("Kunden-IBAN", m.as_ref().map(|m| mask_iban(&m.iban)).unwrap_or_default()),
    ])
}

/// Pre-notification printed on the invoice when its closing text does not contain it.
pub fn prenotification(conn: &Connection, s: &Settings, doc: &Document, closing: &str) -> Result<Option<String>> {
    if doc.payment_method != "debit" || doc.kind == "cancellation" || doc.doc_type != "invoice" {
        return Ok(None);
    }
    let Some(m) = doc.mandate_id.map(|id| db::find::<Mandate>(conn, id)).transpose()?.flatten() else { return Ok(None) };
    if closing.contains(&m.reference) {
        return Ok(None);
    }
    let amount = if doc.status == "draft" { doc.gross_total } else { doc.open_amount() };
    Ok(Some(format!(
        "Zahlung per SEPA-Lastschrift: Den Betrag von {} ziehen wir am {} von Ihrem Konto {} ein. Mandatsreferenz {}, Gläubiger-ID {}.",
        fmt_money(amount),
        crate::util::fmt_date_opt(doc.due_date),
        mask_iban(&m.iban),
        m.reference,
        normalize_iban(&s.debit.creditor_id)
    )))
}

/// Invoices that are not dunned: collected by direct debit and not returned.
pub fn not_dunnable(conn: &Connection) -> Result<HashSet<i64>> {
    let returned = returned_docs(&item_states(conn)?);
    let mut stmt = conn.prepare_cached("SELECT id FROM documents WHERE payment_method = 'debit' AND status IN ('open','partial')")?;
    let ids = stmt.query_map([], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<i64>>>()?;
    Ok(ids.into_iter().filter(|id| !returned.contains(id)).collect())
}

// ---------------------------------------------------------------------------
// Direct debit files
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DebitCandidate {
    pub document_id: i64,
    pub number: String,
    pub contact_name: String,
    pub date: NaiveDate,
    pub due_date: Option<NaiveDate>,
    /// Due date, at the earliest the next possible collection day.
    pub collection_date: NaiveDate,
    #[ts(type = "number")]
    pub amount: Decimal,
    pub mandate_reference: String,
    pub scheme: String,
    pub iban: String,
    /// FRST | RCUR | OOFF
    pub sequence: String,
    /// Why the invoice cannot be collected (empty = ok).
    pub problem: String,
    /// Collection date of a file that already contains the invoice (and did not come back).
    pub pending: Option<NaiveDate>,
    /// The last collection came back (Rücklastschrift).
    pub returned: bool,
}

fn contact_name(doc: &Document, contacts: &HashMap<i64, Contact>) -> String {
    doc.contact_id.and_then(|c| contacts.get(&c)).map(|c| c.display_name()).unwrap_or_else(|| doc.address.lines().next().unwrap_or_default().to_string())
}

fn remittance(doc: &Document) -> String {
    sepa_text(&format!("Rechnung {} vom {}", doc.number, fmt_date(doc.date)), 140)
}

/// Open invoices to be collected by direct debit.
pub fn candidates(conn: &Connection) -> Result<Vec<DebitCandidate>> {
    let s = cfg::load(conn)?;
    let states = item_states(conn)?;
    let usage = usage(&states);
    let pending = pending(&states);
    let returned = returned_docs(&states);
    let contacts: HashMap<i64, Contact> = db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c)).collect();
    let earliest = earliest_collection(today());
    let mut out = Vec::new();
    for d in db::query::<Document>(conn, "WHERE doc_type = 'invoice' AND payment_method = 'debit' AND status IN ('open','partial') ORDER BY due_date, date, id", &[])? {
        let open = d.remaining();
        if open <= Decimal::ZERO || d.money_sign() < Decimal::ZERO {
            continue;
        }
        let m = d.mandate_id.map(|id| db::find::<Mandate>(conn, id)).transpose()?.flatten();
        let collection_date = target_day_from(d.due_date.unwrap_or(d.date)).max(earliest);
        let sequence = match &m {
            Some(m) if !m.recurring => "OOFF",
            Some(m) if usage.get(&m.id).is_none_or(|u| u.collections == 0) => "FRST",
            _ => "RCUR",
        };
        out.push(DebitCandidate {
            document_id: d.id,
            number: d.number.clone(),
            contact_name: contact_name(&d, &contacts),
            date: d.date,
            due_date: d.due_date,
            collection_date,
            amount: open,
            mandate_reference: m.as_ref().map(|m| m.reference.clone()).unwrap_or_default(),
            scheme: m.as_ref().map(|m| m.scheme.clone()).unwrap_or_default(),
            iban: m.as_ref().map(|m| m.iban.clone()).unwrap_or_default(),
            sequence: sequence.into(),
            problem: document_problems(conn, &s, &d)?.join("; "),
            pending: pending.get(&d.id).map(|i| i.collection_date),
            returned: returned.contains(&d.id),
        });
    }
    Ok(out)
}

/// Invoices ready to be collected within the next days (dashboard).
pub fn due_count(conn: &Connection) -> Result<(i64, i64)> {
    let soon = today() + Duration::days(5);
    let c = candidates(conn)?;
    let due = c.iter().filter(|c| c.problem.is_empty() && c.pending.is_none() && !c.returned && c.collection_date <= soon).count() as i64;
    let returned = c.iter().filter(|c| c.returned && c.pending.is_none()).count() as i64;
    Ok((due, returned))
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct DebitRunRequest {
    pub account_id: i64,
    pub document_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DebitRunItemView {
    pub item: DebitRunItem,
    pub document_number: String,
    /// The invoice has been paid in the meantime.
    pub paid: bool,
    pub returned: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DebitRunView {
    pub run: DebitRun,
    pub account_name: String,
    pub items: Vec<DebitRunItemView>,
}

struct Creditor {
    name: String,
    iban: String,
    bic: String,
    id: String,
}

fn debtor_address(c: Option<&Contact>) -> String {
    let Some(c) = c.filter(|c| !c.city.trim().is_empty() && c.country.trim().len() == 2) else { return String::new() };
    let mut x = String::from("<PstlAdr>");
    if !c.street.trim().is_empty() {
        x.push_str(&format!("<StrtNm>{}</StrtNm>", xml_escape(&sepa_text(&c.street, 70))));
    }
    if !c.zip.trim().is_empty() {
        x.push_str(&format!("<PstCd>{}</PstCd>", xml_escape(&sepa_text(&c.zip, 16))));
    }
    x.push_str(&format!("<TwnNm>{}</TwnNm><Ctry>{}</Ctry></PstlAdr>", xml_escape(&sepa_text(&c.city, 35)), c.country.trim().to_uppercase()));
    x
}

fn agent(bic: &str, indent: &str) -> String {
    if bic_valid(bic) {
        format!("{indent}<FinInstnId>\n{indent}  <BICFI>{bic}</BICFI>\n{indent}</FinInstnId>\n")
    } else {
        format!("{indent}<FinInstnId>\n{indent}  <Othr>\n{indent}    <Id>NOTPROVIDED</Id>\n{indent}  </Othr>\n{indent}</FinInstnId>\n")
    }
}

/// pain.008.001.08: one payment information block per scheme, sequence type and collection date.
fn pain008(msg_id: &str, created: &str, cdtr: &Creditor, items: &[(DebitRunItem, String)]) -> String {
    let total: Decimal = items.iter().map(|(i, _)| i.amount).sum();
    let mut groups: Vec<((String, String, NaiveDate), Vec<&(DebitRunItem, String)>)> = Vec::new();
    for it in items {
        let key = (it.0.scheme.clone(), it.0.sequence.clone(), it.0.collection_date);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(it),
            None => groups.push((key, vec![it])),
        }
    }
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str("<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.008.001.08\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n");
    x.push_str("  <CstmrDrctDbtInitn>\n");
    x.push_str(&format!(
        "    <GrpHdr>\n      <MsgId>{msg_id}</MsgId>\n      <CreDtTm>{created}</CreDtTm>\n      <NbOfTxs>{}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n      <InitgPty>\n        <Nm>{}</Nm>\n      </InitgPty>\n    </GrpHdr>\n",
        items.len(),
        amount(total),
        xml_escape(&cdtr.name)
    ));
    for (n, ((scheme, seq, date), group)) in groups.iter().enumerate() {
        let sum: Decimal = group.iter().map(|(i, _)| i.amount).sum();
        x.push_str("    <PmtInf>\n");
        x.push_str(&format!("      <PmtInfId>{}</PmtInfId>\n      <PmtMtd>DD</PmtMtd>\n", sepa_id(&format!("{msg_id}-{}", n + 1))));
        // Single bookings: every collection appears on the statement and is matched to its invoice.
        x.push_str(&format!("      <BtchBookg>false</BtchBookg>\n      <NbOfTxs>{}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n", group.len(), amount(sum)));
        x.push_str(&format!(
            "      <PmtTpInf>\n        <SvcLvl>\n          <Cd>SEPA</Cd>\n        </SvcLvl>\n        <LclInstrm>\n          <Cd>{scheme}</Cd>\n        </LclInstrm>\n        <SeqTp>{seq}</SeqTp>\n      </PmtTpInf>\n"
        ));
        x.push_str(&format!("      <ReqdColltnDt>{date}</ReqdColltnDt>\n"));
        x.push_str(&format!("      <Cdtr>\n        <Nm>{}</Nm>\n      </Cdtr>\n", xml_escape(&cdtr.name)));
        x.push_str(&format!("      <CdtrAcct>\n        <Id>\n          <IBAN>{}</IBAN>\n        </Id>\n      </CdtrAcct>\n", cdtr.iban));
        x.push_str(&format!("      <CdtrAgt>\n{}      </CdtrAgt>\n", agent(&cdtr.bic, "        ")));
        x.push_str("      <ChrgBr>SLEV</ChrgBr>\n");
        x.push_str(&format!(
            "      <CdtrSchmeId>\n        <Id>\n          <PrvtId>\n            <Othr>\n              <Id>{}</Id>\n              <SchmeNm>\n                <Prtry>SEPA</Prtry>\n              </SchmeNm>\n            </Othr>\n          </PrvtId>\n        </Id>\n      </CdtrSchmeId>\n",
            cdtr.id
        ));
        for (it, address) in group {
            x.push_str("      <DrctDbtTxInf>\n");
            x.push_str(&format!("        <PmtId>\n          <EndToEndId>{}</EndToEndId>\n        </PmtId>\n", it.end_to_end_id));
            x.push_str(&format!("        <InstdAmt Ccy=\"EUR\">{}</InstdAmt>\n", amount(it.amount)));
            x.push_str(&format!(
                "        <DrctDbtTx>\n          <MndtRltdInf>\n            <MndtId>{}</MndtId>\n            <DtOfSgntr>{}</DtOfSgntr>\n          </MndtRltdInf>\n        </DrctDbtTx>\n",
                xml_escape(&it.mandate_reference),
                it.mandate_signed_on
            ));
            x.push_str(&format!("        <DbtrAgt>\n{}        </DbtrAgt>\n", agent(&it.debtor_bic, "          ")));
            x.push_str(&format!("        <Dbtr>\n          <Nm>{}</Nm>\n", xml_escape(&it.debtor_name)));
            if !address.is_empty() {
                x.push_str(&format!("          {address}\n"));
            }
            x.push_str("        </Dbtr>\n");
            x.push_str(&format!("        <DbtrAcct>\n          <Id>\n            <IBAN>{}</IBAN>\n          </Id>\n        </DbtrAcct>\n", it.debtor_iban));
            if !it.remittance.is_empty() {
                x.push_str(&format!("        <RmtInf>\n          <Ustrd>{}</Ustrd>\n        </RmtInf>\n", xml_escape(&it.remittance)));
            }
            x.push_str("      </DrctDbtTxInf>\n");
        }
        x.push_str("    </PmtInf>\n");
    }
    x.push_str("  </CstmrDrctDbtInitn>\n</Document>\n");
    x
}

/// Creates a direct debit file for the selected invoices.
pub fn create(conn: &Connection, req: DebitRunRequest) -> Result<DebitRunView> {
    let s = cfg::load(conn)?;
    let Some(account) = db::find::<PaymentAccount>(conn, req.account_id)? else {
        bail_user!("Bitte ein Bankkonto auswählen.");
    };
    if account.kind != "bank" || !iban_valid(&account.iban) {
        bail_user!("Das Konto „{}“ hat keine gültige IBAN (Bank & Zahlungen → Konto bearbeiten).", account.name);
    }
    if !creditor_id_valid(&s.debit.creditor_id) {
        bail_user!("Bitte eine gültige Gläubiger-ID angeben (Einstellungen → Unternehmen).");
    }
    let cdtr = Creditor {
        name: sepa_text(&s.company.name, 70),
        iban: normalize_iban(&account.iban),
        bic: if bic_valid(&account.bic) { normalize_iban(&account.bic) } else { String::new() },
        id: normalize_iban(&s.debit.creditor_id),
    };
    if cdtr.name.is_empty() {
        bail_user!("Bitte den Namen Ihres Unternehmens angeben (Einstellungen → Unternehmen).");
    }
    if req.document_ids.is_empty() {
        bail_user!("Bitte mindestens eine Rechnung auswählen.");
    }
    let candidates: HashMap<i64, DebitCandidate> = candidates(conn)?.into_iter().map(|c| (c.document_id, c)).collect();
    let contacts: HashMap<i64, Contact> = db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c)).collect();
    let mut first_in_run: HashSet<i64> = HashSet::new();
    let mut items = Vec::new();
    for (i, id) in req.document_ids.iter().enumerate() {
        if req.document_ids[..i].contains(id) {
            bail_user!("Eine Rechnung ist doppelt ausgewählt.");
        }
        let Some(c) = candidates.get(id) else {
            bail_user!("Rechnung {id} ist nicht offen oder wird nicht per Lastschrift bezahlt.");
        };
        if !c.problem.is_empty() {
            bail_user!("Rechnung {}: {}.", c.number, c.problem);
        }
        let d: Document = db::get(conn, *id)?;
        let m: Mandate = db::get(conn, d.mandate_id.unwrap_or_default())?;
        // Only the first collection of an unused mandate is FRST, also within one file.
        let sequence = if c.sequence == "FRST" && !first_in_run.insert(m.id) { "RCUR".to_string() } else { c.sequence.clone() };
        let item = DebitRunItem {
            id: 0,
            run_id: 0,
            document_id: d.id,
            mandate_id: m.id,
            amount: c.amount,
            collection_date: c.collection_date,
            sequence,
            scheme: m.scheme.clone(),
            debtor_name: sepa_text(&m.account_holder, 70),
            debtor_iban: normalize_iban(&m.iban),
            debtor_bic: if bic_valid(&m.bic) { normalize_iban(&m.bic) } else { String::new() },
            mandate_reference: m.reference.clone(),
            mandate_signed_on: m.signed_on,
            remittance: remittance(&d),
            end_to_end_id: String::new(),
        };
        let address = if NON_EEA_SEPA.contains(&country(&m.iban)) { debtor_address(d.contact_id.and_then(|c| contacts.get(&c))) } else { String::new() };
        items.push((item, address, d));
    }
    let now = Local::now();
    let mut run = DebitRun {
        id: 0,
        account_id: account.id,
        message_id: String::new(),
        total: items.iter().map(|(i, _, _)| i.amount).sum(),
        count: items.len() as i64,
        created_at: now_str(),
    };
    let id = db::insert(conn, &mut run)?;
    run.message_id = sepa_id(&format!("SOROBO-LS-{}-{id}", now.format("%Y%m%d%H%M%S")));
    db::update(conn, &run)?;
    let mut with_address = Vec::new();
    for (mut item, address, d) in items {
        item.run_id = id;
        // Returns are matched by the stored ID, so runs from before the rename ("SQL…") still are.
        item.end_to_end_id = sepa_id(&format!("SOL{id}-{}", d.number));
        db::insert(conn, &mut item)?;
        cfg::log(conn, "document", d.id, "debit", format!("Lastschrift über {} zum {} erstellt ({})", fmt_money(item.amount), fmt_date(item.collection_date), item.sequence))?;
        with_address.push((item, address));
    }
    let xml = pain008(&run.message_id, &now.format("%Y-%m-%dT%H:%M:%S").to_string(), &cdtr, &with_address);
    conn.execute("UPDATE debit_runs SET xml = ?1 WHERE id = ?2", rusqlite::params![xml, id])?;
    get(conn, id)
}

pub fn get(conn: &Connection, id: i64) -> Result<DebitRunView> {
    let run: DebitRun = db::get(conn, id)?;
    let account_name = db::find::<PaymentAccount>(conn, run.account_id)?.map(|a| a.name).unwrap_or_default();
    let returned: HashSet<i64> = item_states(conn)?.into_iter().filter(|s| s.returned).map(|s| s.item.id).collect();
    let mut items = Vec::new();
    for item in db::query::<DebitRunItem>(conn, "WHERE run_id = ?1 ORDER BY id", &[v(id)])? {
        let d = db::find::<Document>(conn, item.document_id)?;
        items.push(DebitRunItemView {
            document_number: d.as_ref().map(|d| d.number.clone()).unwrap_or_default(),
            paid: d.is_none_or(|d| d.status == "paid"),
            returned: returned.contains(&item.id),
            item,
        });
    }
    Ok(DebitRunView { run, account_name, items })
}

pub fn list(conn: &Connection) -> Result<Vec<DebitRunView>> {
    db::query::<DebitRun>(conn, "ORDER BY id DESC LIMIT 50", &[])?.into_iter().map(|r| get(conn, r.id)).collect()
}

/// (file name, pain.008 XML) of a direct debit file.
pub fn file(conn: &Connection, id: i64) -> Result<(String, String)> {
    let run: DebitRun = db::get(conn, id)?;
    let xml: String = conn.query_row("SELECT xml FROM debit_runs WHERE id = ?1", [id], |r| r.get(0))?;
    Ok((format!("SEPA-Lastschriften_{}_{}.xml", &run.created_at.get(..10).unwrap_or_default(), run.id), xml))
}

/// Discards a direct debit file that was not handed to the bank; the invoices stay open.
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let run = get(conn, id)?;
    if run.items.iter().any(|i| i.paid || i.returned) {
        bail_user!("Aus dieser Datei sind bereits Zahlungen oder Rücklastschriften gebucht – sie wurde also eingereicht und kann nicht verworfen werden.");
    }
    for item in &run.items {
        cfg::log(conn, "document", item.item.document_id, "debit_removed", format!("Lastschriftdatei vom {} verworfen", fmt_date(item.item.collection_date)))?;
    }
    conn.execute("DELETE FROM debit_run_items WHERE run_id = ?1", [id])?;
    conn.execute("DELETE FROM debit_runs WHERE id = ?1", [id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Bank import: batch collections and Rücklastschriften
// ---------------------------------------------------------------------------

/// German text of a SEPA return reason code (ISO 20022 / EPC).
pub fn return_reason(code: &str) -> &'static str {
    match code {
        "AC01" => "IBAN fehlerhaft",
        "AC04" => "Konto erloschen",
        "AC06" => "Konto gesperrt",
        "AC13" => "Konto eines Verbrauchers (Firmenlastschrift nicht möglich)",
        "AG01" => "Lastschrift auf diesem Konto nicht zulässig",
        "AG02" => "ungültige Zahlungsart",
        "AM04" => "Deckung unzureichend",
        "AM05" => "doppelte Lastschrift",
        "BE05" => "Gläubiger unbekannt",
        "FF01" => "ungültiges Dateiformat",
        "MD01" => "kein gültiges Mandat",
        "MD02" => "Mandatsdaten fehlen oder sind falsch",
        "MD06" => "Erstattung auf Verlangen des Zahlers (Widerspruch)",
        "MD07" => "Kontoinhaber verstorben",
        "MS02" => "vom Zahler abgelehnt",
        "MS03" => "Grund nicht angegeben",
        "RR01" | "RR02" | "RR03" | "RR04" => "regulatorische Gründe",
        "SL01" => "Sonderdienstleistung der Bank des Zahlers",
        _ => "",
    }
}

fn normalized(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

const RETURN_WORDS: &[&str] = &["rücklastschrift", "ruecklastschrift", "rückbelastung", "rueckbelastung", "retoure", "rückgabe", "rueckgabe", "rückbuchung", "rueckbuchung", "return", "lastschriftrückgabe", "rückruf"];

/// Collections that may come back with this (outgoing) movement: not returned, the invoice was
/// paid, and the amount fits (the bank may add a fee of up to 50 €). Best match first.
fn return_matches(conn: &Connection, tx: &Transaction) -> Result<Vec<(DebitRunItem, Document, i64)>> {
    if tx.amount >= Decimal::ZERO {
        return Ok(vec![]);
    }
    let purpose = format!("{} {}", tx.purpose, tx.counterparty);
    let text = purpose.to_lowercase();
    let norm = normalized(&purpose);
    let keyword = RETURN_WORDS.iter().any(|w| text.contains(w));
    let iban = normalize_iban(&tx.counterparty_iban);
    let paid = -tx.amount;
    let mut out = Vec::new();
    let states = item_states(conn)?;
    // Only the latest collection of an invoice can come back.
    let mut latest: HashMap<i64, &DebitRunItem> = HashMap::new();
    for s in states.iter().filter(|s| !s.returned) {
        latest.insert(s.item.document_id, &s.item);
    }
    for item in latest.into_values() {
        let fee = paid - item.amount;
        if fee < Decimal::ZERO || fee > Decimal::from(50) {
            continue;
        }
        let Some(d) = db::find::<Document>(conn, item.document_id)? else { continue };
        if d.paid_total < item.amount {
            continue;
        }
        let e2e = normalized(&item.end_to_end_id);
        let score = if e2e.len() >= 6 && norm.contains(&e2e) {
            95
        } else if keyword && (norm.contains(&normalized(&d.number)) || (!iban.is_empty() && iban == item.debtor_iban)) {
            80
        } else if !iban.is_empty() && iban == item.debtor_iban && fee.is_zero() {
            45
        } else {
            continue;
        };
        out.push((item.clone(), d, score));
    }
    out.sort_by(|a, b| b.2.cmp(&a.2).then(b.0.collection_date.cmp(&a.0.collection_date)));
    Ok(out)
}

/// Suggestions for the manual assignment of a return debit (the invoice is reopened).
pub fn return_suggestions(conn: &Connection, tx: &Transaction) -> Result<Vec<MatchSuggestion>> {
    let contacts: HashMap<i64, Contact> = db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c)).collect();
    Ok(return_matches(conn, tx)?
        .into_iter()
        .map(|(item, d, score)| MatchSuggestion {
            kind: "document".into(),
            id: d.id,
            number: d.number.clone(),
            name: contact_name(&d, &contacts),
            date: item.collection_date,
            open_amount: item.amount,
            score,
            reason: format!("Rücklastschrift der Lastschrift vom {}", fmt_date(item.collection_date)),
        })
        .collect())
}

/// Books an imported return debit: the collected amount goes back to the invoice (it is open
/// again), a fee charged by the bank to Nebenkosten des Geldverkehrs.
pub fn match_return(conn: &Connection, tx_id: i64) -> Result<bool> {
    let tx: Transaction = db::get(conn, tx_id)?;
    let matches = return_matches(conn, &tx)?;
    let Some((item, d, score)) = matches.first() else { return Ok(false) };
    if *score < 80 || matches.get(1).is_some_and(|m| m.2 >= 80) {
        return Ok(false);
    }
    let s = cfg::load(conn)?;
    let fee = -tx.amount - item.amount;
    let mut allocations = vec![Allocation { kind: "document".into(), document_id: Some(d.id), amount: -item.amount, date: tx.date, ..Default::default() }];
    if fee > Decimal::ZERO {
        allocations.push(Allocation {
            kind: "account".into(),
            account: crate::chart::role(&s.tax.chart, "geldverkehr"),
            amount: -fee,
            date: tx.date,
            description: format!("Rücklastschriftgebühr {}", d.number),
            ..Default::default()
        });
    }
    finance::update(conn, tx_id, TransactionInput { tx: tx.clone(), allocations })?;
    let code = tx.purpose.split("Rückgabegrund: ").nth(1).and_then(|r| r.split_whitespace().next()).unwrap_or_default();
    let reason = match return_reason(code) {
        "" if code.is_empty() => String::new(),
        "" => format!(" ({code})"),
        r => format!(" ({code}: {r})"),
    };
    let fee_text = if fee > Decimal::ZERO { format!(", Gebühr {}", fmt_money(fee)) } else { String::new() };
    cfg::log(conn, "document", d.id, "debit_returned", format!("Rücklastschrift am {}{reason}{fee_text} – die Rechnung ist wieder offen", fmt_date(tx.date)))?;
    Ok(true)
}

/// Books a collection credited as one amount (Sammlergutschrift) to the invoices of the file:
/// the amount equals the pending collections of one date in one file.
pub fn match_collection(conn: &Connection, tx_id: i64) -> Result<bool> {
    let tx: Transaction = db::get(conn, tx_id)?;
    if tx.amount <= Decimal::ZERO {
        return Ok(false);
    }
    let states = item_states(conn)?;
    let pending = pending(&states);
    let mut groups: HashMap<(i64, NaiveDate), Vec<(&DebitRunItem, Document)>> = HashMap::new();
    for item in pending.values() {
        let Some(d) = db::find::<Document>(conn, item.document_id)? else { continue };
        if d.remaining() != item.amount {
            continue;
        }
        groups.entry((item.run_id, item.collection_date)).or_default().push((item, d));
    }
    let fitting: Vec<_> = groups
        .into_iter()
        .filter(|((_, date), g)| g.len() > 1 && (tx.date - *date).num_days().abs() <= 10 && g.iter().map(|(i, _)| i.amount).sum::<Decimal>() == tx.amount)
        .collect();
    let [(_, group)] = fitting.as_slice() else { return Ok(false) };
    let allocations = group.iter().map(|(i, d)| Allocation { kind: "document".into(), document_id: Some(d.id), amount: i.amount, date: tx.date, ..Default::default() }).collect();
    finance::update(conn, tx_id, TransactionInput { tx: tx.clone(), allocations })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DocumentLine, RecurringInvoice};
    use crate::services::documents::{self, DocumentInput};
    use crate::util::ymd;
    use rust_decimal_macros::dec;

    fn setup() -> (Connection, i64, Contact) {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        cfg::seed_test_company(&conn);
        let mut s = cfg::load(&conn).unwrap();
        s.debit.creditor_id = "DE98ZZZ09999999999".into();
        cfg::save(&conn, &s).unwrap();
        let bank = finance::save_account(
            &conn,
            PaymentAccount { name: "Bank".into(), iban: "DE89 3704 0044 0532 0130 00".into(), bic: "COBADEFFXXX".into(), opening_date: ymd(2020, 1, 1), ..Default::default() },
        )
        .unwrap();
        let mut c = Contact { number: "K1001".into(), company: "Kunde & Co. KG".into(), street: "Weg 1".into(), zip: "12345".into(), city: "Stadt".into(), country: "DE".into(), is_customer: true, ..Default::default() };
        db::insert(&conn, &mut c).unwrap();
        (conn, bank, c)
    }

    fn mandate(conn: &Connection, c: &Contact, recurring: bool) -> Mandate {
        let reference = suggest_reference(conn, c.id).unwrap();
        save_mandate(conn, Mandate { contact_id: c.id, reference, signed_on: today() - Duration::days(30), scheme: "CORE".into(), recurring, iban: "DE02120300000000202051".into(), ..Default::default() }).unwrap().mandate
    }

    fn invoice(conn: &Connection, c: &Contact, gross: Decimal) -> Document {
        let s = cfg::load(conn).unwrap();
        let mut doc = documents::new_template(conn, "invoice", Some(c.id)).unwrap().doc;
        doc.prices_gross = true;
        assert_eq!(doc.payment_method, "debit", "a usable mandate makes direct debit the default");
        assert!(doc.closing_text.contains("{{Mandatsreferenz}}"), "{}", doc.closing_text);
        doc.payment_terms_days = Some(s.debit.prenotification_days);
        doc.due_date = Some(doc.date + Duration::days(s.debit.prenotification_days));
        let line = DocumentLine { line_type: "item".into(), name: "Wartung".into(), quantity: dec!(1), unit_price: gross, tax_rate: dec!(19), ..Default::default() };
        let id = documents::create(conn, DocumentInput { doc, lines: vec![line] }).unwrap();
        documents::finalize(conn, id).unwrap()
    }

    fn import(conn: &Connection, bank: i64, amount: Decimal, purpose: &str, iban: &str) -> i64 {
        let tx = Transaction { account_id: bank, date: today(), amount, purpose: purpose.into(), counterparty_iban: iban.into(), status: "open".into(), source: "import".into(), ..Default::default() };
        finance::create(conn, TransactionInput { tx, allocations: vec![] }).unwrap()
    }

    #[test]
    fn mandates_are_validated_and_expire() {
        let (conn, _, c) = setup();
        assert_eq!(suggest_reference(&conn, c.id).unwrap(), "K1001-1");
        let bad = |m: Mandate| save_mandate(&conn, m).unwrap_err().to_string();
        let base = Mandate { contact_id: c.id, reference: "K1001-1".into(), signed_on: ymd(2026, 1, 1), recurring: true, iban: "DE02120300000000202051".into(), ..Default::default() };
        assert!(bad(Mandate { reference: "Mit Leerzeichen".into(), ..base.clone() }).contains("Mandatsreferenz"));
        assert!(bad(Mandate { iban: "DE02120300000000202052".into(), ..base.clone() }).contains("IBAN"));
        assert!(bad(Mandate { iban: "US12345678901234567".into(), ..base.clone() }).contains("IBAN"));
        let m = save_mandate(&conn, Mandate { signed_on: today() - Duration::days(10), ..base.clone() }).unwrap();
        assert_eq!((m.status.as_str(), m.mandate.scheme.as_str(), m.mandate.account_holder.as_str()), ("active", "CORE", "Kunde & Co. KG"));
        assert!(bad(Mandate { id: 0, ..m.mandate.clone() }).contains("bereits vergeben"));
        assert_eq!(suggest_reference(&conn, c.id).unwrap(), "K1001-2");

        let old = Mandate { signed_on: ymd(2020, 1, 1), ..base.clone() };
        assert_eq!(view(old.clone(), Usage::default(), ymd(2023, 1, 2)).status, "expired");
        assert_eq!(view(old.clone(), Usage { collections: 3, last: Some(ymd(2022, 6, 1)) }, ymd(2023, 1, 2)).status, "active");
        assert_eq!(view(Mandate { recurring: false, ..old.clone() }, Usage { collections: 1, last: Some(ymd(2022, 6, 1)) }, ymd(2023, 1, 2)).status, "used");
        assert_eq!(view(Mandate { revoked_on: Some(ymd(2022, 7, 1)), ..old }, Usage::default(), ymd(2022, 7, 1)).status, "revoked");
    }

    #[test]
    fn invoices_need_mandate_creditor_id_and_prenotification() {
        let (conn, _, c) = setup();
        let m = mandate(&conn, &c, true);
        let mut doc = documents::new_template(&conn, "invoice", Some(c.id)).unwrap().doc;
        assert_eq!((doc.payment_method.as_str(), doc.mandate_id), ("debit", Some(m.id)));
        doc.payment_terms_days = Some(3);
        doc.due_date = Some(doc.date + Duration::days(3));
        let line = DocumentLine { line_type: "item".into(), name: "Wartung".into(), quantity: dec!(1), unit_price: dec!(100), tax_rate: dec!(19), ..Default::default() };
        let id = documents::create(&conn, DocumentInput { doc, lines: vec![line] }).unwrap();
        let err = documents::finalize(&conn, id).unwrap_err().to_string();
        assert!(err.contains("mindestens 14 Tage"), "{err}");
        let mut s = cfg::load(&conn).unwrap();
        s.debit.prenotification_days = 1;
        s.debit.creditor_id = "DE97ZZZ09999999999".into();
        cfg::save(&conn, &s).unwrap();
        let err = documents::finalize(&conn, id).unwrap_err().to_string();
        assert!(err.contains("Gläubiger-ID ist ungültig"), "{err}");
        s.debit.creditor_id = "DE98ZZZ09999999999".into();
        cfg::save(&conn, &s).unwrap();
        let d = documents::finalize(&conn, id).unwrap();
        assert_eq!(d.status, "open");
        // The pre-notification fills the closing text.
        let vars = placeholders(&conn, &s, &d).unwrap();
        assert_eq!(vars[0], ("Mandatsreferenz", m.reference.clone()));
        assert_eq!(vars[2], ("Kunden-IBAN", "DE02 XXXX XXXX XXXX XX20 51".to_string()));
        assert!(prenotification(&conn, &s, &d, "Mandatsreferenz K1001-1").unwrap().is_none());
        assert!(prenotification(&conn, &s, &d, "Bitte überweisen").unwrap().unwrap().contains("Gläubiger-ID DE98ZZZ09999999999"));
        // Credit notes and cancellations are not collected.
        let storno = documents::cancel(&conn, id).unwrap();
        assert_eq!(db::get::<Document>(&conn, storno).unwrap().payment_method, "");
    }

    #[test]
    fn creates_pain008_with_sequence_types() {
        let (conn, bank, c) = setup();
        let m = mandate(&conn, &c, true);
        let a = invoice(&conn, &c, dec!(119));
        let b = invoice(&conn, &c, dec!(59.50));
        let cands = candidates(&conn).unwrap();
        assert_eq!(cands.len(), 2);
        assert!(cands.iter().all(|c| c.problem.is_empty() && c.sequence == "FRST" && c.collection_date >= earliest_collection(today())), "{cands:?}");
        let run = create(&conn, DebitRunRequest { account_id: bank, document_ids: vec![a.id, b.id] }).unwrap();
        assert_eq!((run.run.count, run.run.total), (2, dec!(178.50)));
        let seqs: Vec<&str> = run.items.iter().map(|i| i.item.sequence.as_str()).collect();
        assert_eq!(seqs, vec!["FRST", "RCUR"], "one FRST per mandate");
        let (name, xml) = file(&conn, run.run.id).unwrap();
        assert!(name.starts_with("SEPA-Lastschriften_"));
        for part in [
            "xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.008.001.08\"",
            "<NbOfTxs>2</NbOfTxs>",
            "<CtrlSum>178.50</CtrlSum>",
            "<PmtMtd>DD</PmtMtd>",
            "<Cd>CORE</Cd>",
            "<SeqTp>FRST</SeqTp>",
            "<SeqTp>RCUR</SeqTp>",
            "<Id>DE98ZZZ09999999999</Id>",
            &format!("<MndtId>{}</MndtId>", m.reference),
            "<IBAN>DE02120300000000202051</IBAN>",
            "<Id>NOTPROVIDED</Id>",
            "<Nm>Kunde + Co. KG</Nm>",
            "<InstdAmt Ccy=\"EUR\">59.50</InstdAmt>",
            &format!("<Ustrd>Rechnung {} vom {}</Ustrd>", a.number, fmt_date(a.date)),
        ] {
            assert!(xml.contains(part), "missing {part}\n{xml}");
        }
        let cands = candidates(&conn).unwrap();
        assert!(cands.iter().all(|c| c.pending.is_some()));
        assert_eq!(not_dunnable(&conn).unwrap().len(), 2);
        // Next invoice of the mandate: RCUR.
        invoice(&conn, &c, dec!(10));
        assert_eq!(candidates(&conn).unwrap().iter().find(|c| c.pending.is_none()).unwrap().sequence, "RCUR");
        delete(&conn, run.run.id).unwrap();
        assert!(candidates(&conn).unwrap().iter().all(|c| c.sequence == "FRST" && c.pending.is_none()));
    }

    #[test]
    fn batch_credit_and_return_debit_are_booked() {
        let (conn, bank, c) = setup();
        mandate(&conn, &c, true);
        let a = invoice(&conn, &c, dec!(119));
        let b = invoice(&conn, &c, dec!(59.50));
        let run = create(&conn, DebitRunRequest { account_id: bank, document_ids: vec![a.id, b.id] }).unwrap();
        let date = run.items[0].item.collection_date;

        // The bank credits the whole file at once.
        let mut s = cfg::load(&conn).unwrap();
        s.tax.locked_until = None;
        cfg::save(&conn, &s).unwrap();
        let credit = import(&conn, bank, dec!(178.50), "SEPA-Sammlergutschrift", "");
        conn.execute("UPDATE transactions SET date = ?1 WHERE id = ?2", rusqlite::params![date.to_string(), credit]).unwrap();
        assert!(match_collection(&conn, credit).unwrap());
        assert_eq!(db::get::<Document>(&conn, a.id).unwrap().status, "paid");
        assert_eq!(db::get::<Document>(&conn, b.id).unwrap().status, "paid");

        // One collection comes back with a fee of 3 €.
        let e2e = &run.items[1].item.end_to_end_id;
        let ret = import(&conn, bank, dec!(-62.50), &format!("RUECKLASTSCHRIFT EREF: {e2e} Rückgabegrund: AM04"), "DE02120300000000202051");
        assert_eq!(return_suggestions(&conn, &db::get(&conn, ret).unwrap()).unwrap()[0].id, b.id);
        assert!(match_return(&conn, ret).unwrap());
        let b2: Document = db::get(&conn, b.id).unwrap();
        assert_eq!((b2.status.as_str(), b2.open_amount()), ("open", dec!(59.50)));
        let tx: Transaction = db::get(&conn, ret).unwrap();
        assert_eq!(tx.status, "booked");
        let log = cfg::activities(&conn, "document", b.id).unwrap();
        assert!(log.iter().any(|l| l.message.contains("AM04: Deckung unzureichend") && l.message.contains("Gebühr 3,00")), "{log:?}");

        // The invoice can be collected again; the mandate stays in use (RCUR).
        let cands = candidates(&conn).unwrap();
        let again = cands.iter().find(|c| c.document_id == b.id).unwrap();
        assert!(again.returned && again.pending.is_none() && again.sequence == "RCUR", "{again:?}");
        assert_eq!(due_count(&conn).unwrap().1, 1);
        assert!(!not_dunnable(&conn).unwrap().contains(&b.id), "returned invoices can be dunned");
        assert!(delete(&conn, run.run.id).is_err(), "a file with bookings cannot be discarded");
    }

    #[test]
    fn recurring_invoices_keep_the_mandate() {
        let (conn, _, c) = setup();
        let m = mandate(&conn, &c, true);
        let t = invoice(&conn, &c, dec!(119));
        crate::services::recurring::save(&conn, RecurringInvoice { template_id: t.id, cycle: "monthly".into(), next_date: today(), auto_finalize: true, active: true, ..Default::default() }).unwrap();
        let created = crate::services::recurring::create_due(&conn, today()).unwrap();
        let d: Document = db::get(&conn, created[0].document_id).unwrap();
        assert!(created[0].finalized);
        assert_eq!((d.payment_method.as_str(), d.mandate_id), ("debit", Some(m.id)));
    }
}
