// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Sales documents: quotes, order confirmations, delivery notes, invoices,
//! partial / final invoices, credit notes and cancellations.

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;
use rust_decimal::prelude::Signed;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::{Activity, Allocation, Contact, Document, DocumentLine, Dunning, PaymentAccount, Settings, Transaction};
use crate::services::calc::{self, Totals};
use crate::services::{posting, settings as cfg};
use crate::util::{self, HUNDRED, fmt_date, fmt_money, now_str, r2, today};

pub const DOC_TYPES: &[&str] = &["quote", "order", "delivery", "invoice", "credit_note"];

pub fn type_label(doc_type: &str, kind: &str) -> &'static str {
    match (doc_type, kind) {
        ("invoice", "partial") => "Abschlagsrechnung",
        ("invoice", "final") => "Schlussrechnung",
        ("invoice", "cancellation") => "Stornorechnung",
        ("invoice", _) => "Rechnung",
        ("credit_note", _) => "Gutschrift",
        ("quote", _) => "Angebot",
        ("order", _) => "Auftragsbestätigung",
        ("delivery", _) => "Lieferschein",
        _ => "Dokument",
    }
}

fn text_key(doc: &Document) -> String {
    match (doc.doc_type.as_str(), doc.kind.as_str()) {
        ("invoice", "partial") => "partial".into(),
        ("invoice", "final") => "final".into(),
        ("invoice", "cancellation") => "cancellation".into(),
        (t, _) => t.into(),
    }
}

fn range_key(doc: &Document) -> &'static str {
    match (doc.doc_type.as_str(), doc.kind.as_str()) {
        ("invoice", "cancellation") => "cancellation",
        ("invoice", _) => "invoice",
        ("credit_note", _) => "credit_note",
        ("quote", _) => "quote",
        ("order", _) => "order",
        _ => "delivery",
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct DocumentInput {
    pub doc: Document,
    pub lines: Vec<DocumentLine>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PaymentInfo {
    pub allocation: Allocation,
    pub transaction: Option<Transaction>,
    pub account_name: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RelatedDoc {
    pub id: i64,
    pub doc_type: String,
    pub kind: String,
    pub number: String,
    pub status: String,
    pub date: NaiveDate,
    #[ts(type = "number")]
    pub net_total: Decimal,
    #[ts(type = "number")]
    pub gross_total: Decimal,
    /// source | derived | partial | final | cancellation | base
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DocumentFull {
    pub doc: Document,
    pub lines: Vec<DocumentLine>,
    pub totals: Totals,
    pub contact: Option<Contact>,
    pub payments: Vec<PaymentInfo>,
    pub dunnings: Vec<Dunning>,
    pub related: Vec<RelatedDoc>,
    pub activities: Vec<Activity>,
    #[ts(type = "number")]
    pub open_amount: Decimal,
    pub days_overdue: i64,
    #[ts(type = "number")]
    pub dunning_fees: Decimal,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DocumentListItem {
    pub id: i64,
    pub doc_type: String,
    pub kind: String,
    pub status: String,
    pub number: String,
    pub contact_id: Option<i64>,
    pub contact_name: String,
    pub date: NaiveDate,
    pub due_date: Option<NaiveDate>,
    pub valid_until: Option<NaiveDate>,
    pub title: String,
    #[ts(type = "number")]
    pub net_total: Decimal,
    #[ts(type = "number")]
    pub gross_total: Decimal,
    #[ts(type = "number")]
    pub paid_total: Decimal,
    #[ts(type = "number")]
    pub open_amount: Decimal,
    pub days_overdue: i64,
    pub dunning_level: i64,
    pub dunning_count: i64,
    pub einvoice_format: String,
    pub sent: bool,
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct DocumentFilter {
    pub doc_type: String,
    pub status: String,
    pub q: String,
    pub contact_id: Option<i64>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

pub fn lines_of(conn: &Connection, doc_id: i64) -> Result<Vec<DocumentLine>> {
    db::query::<DocumentLine>(conn, "WHERE document_id = ?1 ORDER BY position, id", &[v(doc_id)])
}

pub fn days_overdue(doc: &Document) -> i64 {
    if doc.open_amount() <= Decimal::ZERO || doc.doc_type != "invoice" {
        return 0;
    }
    match doc.due_date {
        Some(d) if d < today() => (today() - d).num_days(),
        _ => 0,
    }
}

pub fn list(conn: &Connection, f: &DocumentFilter) -> Result<Vec<DocumentListItem>> {
    let mut sql = String::from("WHERE 1=1");
    let mut params = Vec::new();
    if !f.doc_type.is_empty() {
        params.push(v(f.doc_type.clone()));
        sql.push_str(&format!(" AND doc_type = ?{}", params.len()));
    }
    if let Some(c) = f.contact_id {
        params.push(v(c));
        sql.push_str(&format!(" AND contact_id = ?{}", params.len()));
    }
    if let Some(d) = f.from {
        params.push(v(d));
        sql.push_str(&format!(" AND date >= ?{}", params.len()));
    }
    if let Some(d) = f.to {
        params.push(v(d));
        sql.push_str(&format!(" AND date <= ?{}", params.len()));
    }
    sql.push_str(" ORDER BY date DESC, id DESC");
    let docs = db::query::<Document>(conn, &sql, &params)?;
    let contacts = contact_names(conn)?;
    let dunning_counts = dunning_counts(conn)?;
    let q = f.q.trim().to_lowercase();
    let mut out = Vec::new();
    for d in docs {
        let contact_name = d
            .contact_id
            .and_then(|id| contacts.get(&id).cloned())
            .unwrap_or_else(|| d.address.lines().next().unwrap_or_default().to_string());
        if !q.is_empty() {
            let hay = format!("{} {} {} {}", d.number, contact_name, d.title, d.reference).to_lowercase();
            if !hay.contains(&q) {
                continue;
            }
        }
        let overdue = days_overdue(&d);
        let status_ok = match f.status.as_str() {
            "" | "all" => true,
            "overdue" => overdue > 0,
            "unpaid" => d.remaining() > Decimal::ZERO,
            s => d.status == s,
        };
        if !status_ok {
            continue;
        }
        out.push(DocumentListItem {
            id: d.id,
            doc_type: d.doc_type.clone(),
            kind: d.kind.clone(),
            status: d.status.clone(),
            number: d.number.clone(),
            contact_id: d.contact_id,
            contact_name,
            date: d.date,
            due_date: d.due_date,
            valid_until: d.valid_until,
            title: d.title.clone(),
            net_total: d.net_total,
            gross_total: d.gross_total,
            paid_total: d.paid_total,
            open_amount: d.open_amount(),
            days_overdue: overdue,
            dunning_level: d.dunning_level,
            dunning_count: dunning_counts.get(&d.id).copied().unwrap_or(0),
            einvoice_format: d.einvoice_format.clone(),
            sent: !d.sent_at.is_empty(),
        });
    }
    Ok(out)
}

fn contact_names(conn: &Connection) -> Result<std::collections::HashMap<i64, String>> {
    Ok(db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c.display_name())).collect())
}

fn dunning_counts(conn: &Connection) -> Result<std::collections::HashMap<i64, i64>> {
    let mut stmt = conn.prepare("SELECT document_id, COUNT(*) FROM dunnings GROUP BY document_id")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_full(conn: &Connection, id: i64) -> Result<DocumentFull> {
    let doc: Document = db::get(conn, id)?;
    let mut lines = lines_of(conn, id)?;
    let totals = calc::calculate(&doc, &mut lines);
    let contact = match doc.contact_id {
        Some(cid) => db::find::<Contact>(conn, cid)?,
        None => None,
    };
    let allocations = db::query::<Allocation>(conn, "WHERE document_id = ?1 ORDER BY date, id", &[v(id)])?;
    let mut payments = Vec::new();
    for a in allocations {
        let tx = match a.transaction_id {
            Some(t) => db::find::<Transaction>(conn, t)?,
            None => None,
        };
        let account_name = match &tx {
            Some(t) => db::find::<PaymentAccount>(conn, t.account_id)?.map(|p| p.name).unwrap_or_default(),
            None => match a.writeoff_reason.as_str() {
                "skonto" => "Skonto".into(),
                "loss" => "Forderungsverlust".into(),
                _ => "Ausbuchung".into(),
            },
        };
        payments.push(PaymentInfo { allocation: a, transaction: tx, account_name });
    }
    let dunnings = db::query::<Dunning>(conn, "WHERE document_id = ?1 ORDER BY level, id", &[v(id)])?;
    let dunning_fees = dunnings.iter().map(|d| d.fee + d.interest).max().unwrap_or_default();
    let related = related_docs(conn, &doc)?;
    let activities = cfg::activities(conn, "document", id)?;
    Ok(DocumentFull {
        open_amount: doc.open_amount(),
        days_overdue: days_overdue(&doc),
        doc,
        lines,
        totals,
        contact,
        payments,
        dunnings,
        related,
        activities,
        dunning_fees,
    })
}

fn related_docs(conn: &Connection, doc: &Document) -> Result<Vec<RelatedDoc>> {
    let mut out: Vec<RelatedDoc> = Vec::new();
    let mut push = |d: Document, relation: &str| {
        if d.id != doc.id && !out.iter().any(|r| r.id == d.id) {
            out.push(RelatedDoc {
                id: d.id,
                doc_type: d.doc_type,
                kind: d.kind,
                number: d.number,
                status: d.status,
                date: d.date,
                net_total: d.net_total,
                gross_total: d.gross_total,
                relation: relation.into(),
            });
        }
    };
    if let Some(s) = doc.source_id
        && let Some(d) = db::find::<Document>(conn, s)?
    {
        push(d, "source");
    }
    if let Some(b) = doc.base_id
        && let Some(d) = db::find::<Document>(conn, b)?
    {
        push(d, "base");
    }
    if let Some(c) = doc.cancels_id
        && let Some(d) = db::find::<Document>(conn, c)?
    {
        push(d, "cancelled");
    }
    // Partial and final invoices before other derived documents – they also carry source_id.
    let base = doc.base_id.unwrap_or(doc.id);
    for d in db::query::<Document>(conn, "WHERE base_id = ?1 ORDER BY date, id", &[v(base)])? {
        let rel = if d.kind == "final" { "final" } else { "partial" };
        push(d, rel);
    }
    for d in db::query::<Document>(conn, "WHERE source_id = ?1", &[v(doc.id)])? {
        push(d, "derived");
    }
    for d in db::query::<Document>(conn, "WHERE cancels_id = ?1", &[v(doc.id)])? {
        push(d, "cancellation");
    }
    out.sort_by_key(|r| (r.date, r.id));
    Ok(out)
}

/// Suggested tax mode for a contact.
pub fn suggest_tax_mode(settings: &Settings, contact: Option<&Contact>) -> String {
    if settings.tax.small_business {
        return "small_business".into();
    }
    if let Some(c) = contact {
        if !c.default_tax_mode.is_empty() {
            return c.default_tax_mode.clone();
        }
        let country = if c.country.is_empty() { "DE".to_string() } else { c.country.to_uppercase() };
        if country != settings.company.country.to_uppercase() && !country.is_empty() {
            if util::is_eu(&country) {
                return if c.vat_id.trim().is_empty() { "standard".into() } else { "eu_service".into() };
            }
            return "third_country".into();
        }
    }
    "standard".into()
}

/// A prefilled new document (default texts, dates, terms).
pub fn new_template(conn: &Connection, doc_type: &str, contact_id: Option<i64>) -> Result<DocumentInput> {
    if !DOC_TYPES.contains(&doc_type) {
        bail_user!("Unbekannter Dokumenttyp {doc_type}");
    }
    let s = cfg::load(conn)?;
    let mut doc = Document {
        doc_type: doc_type.into(),
        kind: "standard".into(),
        status: "draft".into(),
        date: today(),
        currency: "EUR".into(),
        tax_mode: suggest_tax_mode(&s, None),
        layout_id: s.docs.default_layout_id,
        ..Default::default()
    };
    if doc.is_invoice_like() {
        doc.payment_terms_days = Some(s.docs.payment_terms_days);
        doc.due_date = Some(doc.date + Duration::days(s.docs.payment_terms_days));
        doc.service_date = Some(doc.date);
    }
    if doc_type == "quote" {
        doc.valid_until = Some(doc.date + Duration::days(s.docs.quote_validity_days));
    }
    if doc_type == "delivery" || doc_type == "order" {
        doc.service_date = Some(doc.date);
    }
    let texts = s.docs.texts.get(&text_key(&doc)).cloned().unwrap_or_default();
    doc.title = if texts.title.is_empty() { type_label(doc_type, "standard").into() } else { texts.title };
    doc.intro_text = texts.intro;
    doc.closing_text = texts.closing;
    doc.einvoice_format = s.docs.einvoice_default.clone();
    if let Some(cid) = contact_id {
        let c: Contact = db::get(conn, cid)?;
        apply_contact(&s, &mut doc, &c);
        crate::services::debit::apply_default(conn, &s, &mut doc)?;
    }
    Ok(DocumentInput { doc, lines: vec![] })
}

/// Exchanges the default closing text of invoices when the payment method changes (transfer
/// request ↔ pre-notification of the direct debit); an edited text is kept.
pub fn swap_closing(s: &Settings, doc: &mut Document, method: &str) {
    if doc.doc_type != "invoice" || doc.kind != "standard" {
        return;
    }
    let text = |k: &str| s.docs.texts.get(k).map(|t| t.closing.clone()).unwrap_or_default();
    let (transfer, debit) = (text("invoice"), text("invoice_debit"));
    if method == "debit" && doc.closing_text == transfer {
        doc.closing_text = debit;
    } else if method != "debit" && doc.closing_text == debit {
        doc.closing_text = transfer;
    }
}

pub fn apply_contact(s: &Settings, doc: &mut Document, c: &Contact) {
    doc.contact_id = Some(c.id);
    let person = c
        .primary_person()
        .map(|p| format!("{} {} {} {}", p.salutation, p.title, p.first_name, p.last_name).split_whitespace().collect::<Vec<_>>().join(" "));
    doc.contact_person = person.clone().unwrap_or_default();
    doc.address = c.address_block(person.as_deref(), &s.company.country);
    doc.tax_mode = suggest_tax_mode(s, Some(c));
    doc.buyer_reference = c.leitweg_id.clone();
    if !c.einvoice_format.is_empty() {
        doc.einvoice_format = c.einvoice_format.clone();
    }
    if doc.is_invoice_like()
        && let Some(days) = c.payment_terms_days
    {
        doc.payment_terms_days = Some(days);
        doc.due_date = Some(doc.date + Duration::days(days));
    }
}

fn normalize(s: &Settings, doc: &mut Document, lines: &mut [DocumentLine]) -> Result<()> {
    if !DOC_TYPES.contains(&doc.doc_type.as_str()) {
        bail_user!("Unbekannter Dokumenttyp");
    }
    if doc.kind.is_empty() {
        doc.kind = "standard".into();
    }
    if doc.currency.is_empty() {
        doc.currency = "EUR".into();
    }
    if doc.tax_mode.is_empty() {
        doc.tax_mode = "standard".into();
    }
    if s.tax.small_business {
        doc.tax_mode = "small_business".into();
    }
    if doc.title.trim().is_empty() {
        doc.title = type_label(&doc.doc_type, &doc.kind).into();
    }
    if doc.is_invoice_like()
        && let Some(days) = doc.payment_terms_days
        && doc.due_date.is_none()
    {
        doc.due_date = Some(doc.date + Duration::days(days));
    }
    if doc.doc_type != "invoice" || doc.kind == "cancellation" || doc.payment_method != "debit" {
        doc.payment_method = String::new();
        doc.mandate_id = None;
    } else if let Some(due) = doc.due_date {
        // Direct debits are collected on TARGET2 business days only.
        let collection = util::target_day_from(due);
        if collection != due {
            doc.due_date = Some(collection);
            doc.payment_terms_days = Some((collection - doc.date).num_days());
        }
    }
    for (i, l) in lines.iter_mut().enumerate() {
        l.position = i as i64 + 1;
        if l.line_type.is_empty() {
            l.line_type = "item".into();
        }
        if l.quantity.is_zero() && l.line_type == "item" && l.unit_price.is_zero() {
            l.quantity = Decimal::ONE;
        }
    }
    let t = calc::calculate(doc, lines);
    calc::apply_totals(doc, &t);
    Ok(())
}

fn save_lines(conn: &Connection, doc_id: i64, lines: &mut [DocumentLine]) -> Result<()> {
    conn.execute("DELETE FROM document_lines WHERE document_id = ?1", [doc_id])?;
    for l in lines.iter_mut() {
        l.id = 0;
        l.document_id = doc_id;
        db::insert(conn, l)?;
    }
    Ok(())
}

pub fn create(conn: &Connection, input: DocumentInput) -> Result<i64> {
    let s = cfg::load(conn)?;
    let DocumentInput { mut doc, mut lines } = input;
    doc.id = 0;
    doc.status = "draft".into();
    doc.number = String::new();
    doc.paid_total = Decimal::ZERO;
    doc.finalized_at = String::new();
    doc.created_at = now_str();
    doc.updated_at = now_str();
    normalize(&s, &mut doc, &mut lines)?;
    let id = db::insert(conn, &mut doc)?;
    save_lines(conn, id, &mut lines)?;
    cfg::log(conn, "document", id, "created", format!("{} als Entwurf erstellt", type_label(&doc.doc_type, &doc.kind)))?;
    Ok(id)
}

pub fn update(conn: &Connection, id: i64, input: DocumentInput) -> Result<()> {
    let s = cfg::load(conn)?;
    let old: Document = db::get(conn, id)?;
    if old.is_invoice_like() && old.status != "draft" {
        bail_user!("Abgeschlossene Rechnungen können nicht mehr geändert werden (GoBD). Bitte stornieren oder eine Gutschrift erstellen.");
    }
    let DocumentInput { mut doc, mut lines } = input;
    doc.id = id;
    doc.doc_type = old.doc_type.clone();
    doc.kind = old.kind.clone();
    doc.status = old.status.clone();
    doc.number = old.number.clone();
    doc.paid_total = old.paid_total;
    doc.finalized_at = old.finalized_at.clone();
    doc.sent_at = old.sent_at.clone();
    doc.base_id = old.base_id;
    doc.cancels_id = old.cancels_id;
    doc.source_id = old.source_id;
    doc.dunning_level = old.dunning_level;
    doc.created_at = old.created_at.clone();
    doc.updated_at = now_str();
    normalize(&s, &mut doc, &mut lines)?;
    db::update(conn, &doc)?;
    save_lines(conn, id, &mut lines)?;
    cfg::log(conn, "document", id, "updated", "Dokument bearbeitet")?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let doc: Document = db::get(conn, id)?;
    if doc.is_invoice_like() && doc.status != "draft" {
        bail_user!("Abgeschlossene Rechnungen dürfen nicht gelöscht werden (GoBD). Bitte stattdessen stornieren.");
    }
    let refs = db::scalar_i64(conn, "SELECT COUNT(*) FROM allocations WHERE document_id = ?1", &[v(id)])?;
    if refs > 0 {
        bail_user!("Dem Dokument sind Zahlungen zugeordnet.");
    }
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM time_entries WHERE document_id = ?1", &[v(id)])? > 0 {
        bail_user!("Zu diesem Dokument sind Stunden erfasst. Bitte zuerst die Zeiteinträge löschen oder einem anderen Dokument zuordnen.");
    }
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM recurring_invoices WHERE template_id = ?1", &[v(id)])? > 0 {
        bail_user!("Der Entwurf ist die Vorlage einer Abo-Rechnung. Bitte zuerst die Abo-Rechnung löschen.");
    }
    conn.execute("UPDATE documents SET source_id = NULL WHERE source_id = ?1", [id])?;
    conn.execute("UPDATE documents SET base_id = NULL WHERE base_id = ?1 AND status = 'draft'", [id])?;
    if db::scalar_i64(conn, "SELECT COUNT(*) FROM documents WHERE base_id = ?1", &[v(id)])? > 0 {
        bail_user!("Zu diesem Dokument existieren bereits Abschlags- oder Schlussrechnungen.");
    }
    conn.execute("DELETE FROM activities WHERE entity = 'document' AND entity_id = ?1", [id])?;
    db::delete::<Document>(conn, id)?;
    posting::rebuild(conn, posting::Source::Document(id))?;
    Ok(())
}

pub fn finalize(conn: &Connection, id: i64) -> Result<Document> {
    issue(conn, id, true)
}

/// Demo documents are issued even while the company profile is still incomplete.
pub(crate) fn finalize_demo(conn: &Connection, id: i64) -> Result<Document> {
    issue(conn, id, false)
}

fn issue(conn: &Connection, id: i64, check_invoice_data: bool) -> Result<Document> {
    let s = cfg::load(conn)?;
    let mut doc: Document = db::get(conn, id)?;
    if doc.status != "draft" {
        bail_user!("Das Dokument ist bereits abgeschlossen.");
    }
    let mut lines = lines_of(conn, id)?;
    if doc.address.trim().is_empty() {
        bail_user!("Bitte einen Empfänger bzw. eine Anschrift angeben.");
    }
    if !lines.iter().any(|l| l.line_type == "item" || l.line_type == "deduction") {
        bail_user!("Das Dokument enthält keine Positionen.");
    }
    if doc.is_invoice_like() {
        cfg::check_unlocked(conn, doc.date)?;
        if doc.service_date.is_none() {
            doc.service_date = Some(doc.date);
        }
        if doc.due_date.is_none() {
            doc.due_date = Some(doc.date + Duration::days(doc.payment_terms_days.unwrap_or(s.docs.payment_terms_days)));
        }
        if doc.kind == "final" {
            check_final_partials(conn, &doc, &lines)?;
        }
    }
    let t = calc::calculate(&doc, &mut lines);
    calc::apply_totals(&mut doc, &t);
    if doc.is_invoice_like() && check_invoice_data {
        let missing = missing_invoice_data(conn, &s, &doc, &lines)?;
        if !missing.is_empty() {
            bail_user!("Für eine ordnungsgemäße Rechnung (§ 14 UStG) fehlt: {}.", missing.join("; "));
        }
    }
    if doc.payment_method == "debit" {
        crate::services::debit::check_issue(conn, &s, &doc)?;
    }
    doc.number = cfg::next_number(conn, range_key(&doc), doc.date)?;
    doc.status = "open".into();
    doc.finalized_at = now_str();
    doc.updated_at = now_str();
    db::update(conn, &doc)?;
    save_lines(conn, id, &mut lines)?;
    cfg::log(conn, "document", id, "finalized", format!("Abgeschlossen als {}", doc.number))?;
    if doc.is_invoice_like() {
        // Mark the source quote/order as done once an invoice was issued for it.
        if let Some(src) = doc.source_id
            && let Some(mut sd) = db::find::<Document>(conn, src)?
            && (sd.status == "open" || sd.status == "accepted")
            && (sd.doc_type == "order" || sd.doc_type == "quote")
            && doc.kind != "partial"
        {
            sd.status = if sd.doc_type == "quote" { "accepted".into() } else { "done".into() };
            db::update(conn, &sd)?;
        }
        recompute_paid(conn, id)?;
        posting::rebuild(conn, posting::Source::Document(id))?;
    }
    Ok(db::get(conn, id)?)
}

/// Mandatory information of an invoice (§ 14 Abs. 4 UStG). A Kleinbetragsrechnung up to 250 €
/// gross needs neither the recipient nor the tax number (§ 33 UStDV).
pub fn missing_invoice_data(conn: &Connection, s: &Settings, doc: &Document, lines: &[DocumentLine]) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    let c = &s.company;
    if [&c.name, &c.street, &c.zip, &c.city].iter().any(|f| f.trim().is_empty()) {
        missing.push("Name und Anschrift Ihres Unternehmens (Einstellungen → Unternehmen)".to_string());
    }
    if doc.gross_total.abs() > Decimal::from(250) {
        if c.tax_number.trim().is_empty() && c.vat_id.trim().is_empty() {
            missing.push("Steuernummer oder USt-IdNr. Ihres Unternehmens (Einstellungen → Unternehmen)".into());
        }
        if doc.address.lines().filter(|l| !l.trim().is_empty()).count() < 2 {
            missing.push("vollständige Anschrift des Empfängers".into());
        }
    }
    if lines.iter().any(|l| l.line_type == "item" && l.name.trim().is_empty() && l.description.trim().is_empty()) {
        missing.push("Bezeichnung jeder Position".into());
    }
    if matches!(doc.tax_mode.as_str(), "eu_delivery" | "eu_service") {
        let vat_id = match doc.contact_id {
            Some(id) => db::find::<Contact>(conn, id)?.map(|k| k.vat_id).unwrap_or_default(),
            None => String::new(),
        };
        if vat_id.trim().is_empty() {
            missing.push("USt-IdNr. des Kunden (innergemeinschaftliche Leistung)".into());
        }
    }
    Ok(missing)
}

fn check_final_partials(conn: &Connection, doc: &Document, lines: &[DocumentLine]) -> Result<()> {
    let Some(base) = doc.base_id else { return Ok(()) };
    let partials = db::query::<Document>(
        conn,
        "WHERE base_id = ?1 AND kind = 'partial' AND status NOT IN ('draft','cancelled')",
        &[v(base)],
    )?;
    for p in partials {
        if !lines.iter().any(|l| l.ref_document_id == Some(p.id)) {
            bail_user!("Die Abschlagsrechnung {} ist in der Schlussrechnung nicht berücksichtigt. Bitte Schlussrechnung neu erstellen.", p.number);
        }
    }
    Ok(())
}

pub fn set_status(conn: &Connection, id: i64, status: &str) -> Result<Document> {
    let mut doc: Document = db::get(conn, id)?;
    let allowed: &[&str] = match doc.doc_type.as_str() {
        "quote" => &["open", "accepted", "rejected"],
        "order" | "delivery" => &["open", "done"],
        _ => &[],
    };
    if doc.status == "draft" || !allowed.contains(&status) {
        bail_user!("Statuswechsel nicht möglich.");
    }
    doc.status = status.into();
    doc.updated_at = now_str();
    db::update(conn, &doc)?;
    let label = match status {
        "accepted" => "Als angenommen markiert",
        "rejected" => "Als abgelehnt markiert",
        "done" => "Als erledigt markiert",
        _ => "Wieder geöffnet",
    };
    cfg::log(conn, "document", id, "status", label)?;
    Ok(doc)
}

pub fn mark_sent(conn: &Connection, id: i64, via: &str) -> Result<()> {
    let mut doc: Document = db::get(conn, id)?;
    if doc.status == "draft" {
        bail_user!("Bitte das Dokument zuerst abschließen.");
    }
    doc.sent_at = now_str();
    db::update(conn, &doc)?;
    cfg::log(conn, "document", id, "sent", format!("Als versendet markiert ({via})"))?;
    Ok(())
}

fn clone_lines(lines: &[DocumentLine], negate: bool, drop_deductions: bool) -> Vec<DocumentLine> {
    lines
        .iter()
        .filter(|l| !(drop_deductions && l.line_type == "deduction"))
        .map(|l| {
            let mut n = l.clone();
            n.id = 0;
            n.document_id = 0;
            if negate {
                if n.line_type == "deduction" {
                    n.net_amount = -n.net_amount;
                    n.tax_amount = -n.tax_amount;
                } else {
                    n.quantity = -n.quantity;
                }
            }
            n
        })
        .collect()
}

fn derived_doc(conn: &Connection, src: &Document, doc_type: &str, kind: &str) -> Result<Document> {
    let s = cfg::load(conn)?;
    let tpl = new_template(conn, doc_type, None)?.doc;
    let mut d = src.clone();
    d.id = 0;
    d.doc_type = doc_type.into();
    d.kind = kind.into();
    d.status = "draft".into();
    d.number = String::new();
    d.date = today();
    d.paid_total = Decimal::ZERO;
    d.paid_at = None;
    d.finalized_at = String::new();
    d.sent_at = String::new();
    d.dunning_level = 0;
    d.source_id = Some(src.id);
    d.base_id = None;
    d.cancels_id = None;
    d.partial_mode = String::new();
    d.partial_value = Decimal::ZERO;
    let texts = s.docs.texts.get(&text_key(&d)).cloned().unwrap_or_default();
    d.title = if texts.title.is_empty() { type_label(doc_type, kind).into() } else { texts.title };
    d.intro_text = texts.intro;
    d.closing_text = texts.closing;
    d.valid_until = tpl.valid_until;
    if d.is_invoice_like() {
        let days = src.payment_terms_days.unwrap_or(s.docs.payment_terms_days);
        d.payment_terms_days = Some(days);
        d.due_date = Some(d.date + Duration::days(days));
        if d.service_date.is_none() {
            d.service_date = Some(d.date);
        }
    } else {
        d.due_date = None;
        d.payment_terms_days = None;
    }
    if doc_type != "quote" {
        d.valid_until = None;
    }
    if src.doc_type == "invoice" {
        let method = d.payment_method.clone();
        swap_closing(&s, &mut d, &method);
    } else {
        crate::services::debit::apply_default(conn, &s, &mut d)?;
    }
    d.created_at = now_str();
    d.updated_at = now_str();
    Ok(d)
}

pub fn copy(conn: &Connection, id: i64) -> Result<i64> {
    let src: Document = db::get(conn, id)?;
    let lines = lines_of(conn, id)?;
    let kind = if src.kind == "cancellation" || src.kind == "final" || src.kind == "partial" { "standard" } else { src.kind.as_str() };
    let mut d = derived_doc(conn, &src, &src.doc_type, kind)?;
    d.source_id = None;
    let new_lines = clone_lines(&lines, false, true);
    let new_id = create(conn, DocumentInput { doc: d, lines: new_lines })?;
    cfg::log(conn, "document", new_id, "copied", format!("Kopie von {}", if src.number.is_empty() { "Entwurf" } else { &src.number }))?;
    Ok(new_id)
}

pub fn convert(conn: &Connection, id: i64, target: &str) -> Result<i64> {
    let src: Document = db::get(conn, id)?;
    if !DOC_TYPES.contains(&target) {
        bail_user!("Unbekannter Zieltyp");
    }
    if target == "invoice"
        && db::scalar_i64(conn, "SELECT COUNT(*) FROM documents WHERE base_id = ?1 AND kind IN ('partial','final') AND status NOT IN ('draft','cancelled')", &[v(id)])? > 0
    {
        bail_user!("Zu diesem Auftrag gibt es bereits Abschlagsrechnungen – bitte eine Schlussrechnung erstellen, damit nichts doppelt berechnet wird.");
    }
    let lines = lines_of(conn, id)?;
    let d = derived_doc(conn, &src, target, "standard")?;
    let new_id = create(conn, DocumentInput { doc: d, lines: clone_lines(&lines, false, true) })?;
    cfg::log(conn, "document", new_id, "converted", format!("Erstellt aus {} {}", type_label(&src.doc_type, &src.kind), src.number))?;
    cfg::log(conn, "document", id, "converted", format!("{} erstellt", type_label(target, "standard")))?;
    if src.doc_type == "quote" && src.status == "open" {
        let mut s = src.clone();
        s.status = "accepted".into();
        db::update(conn, &s)?;
    }
    Ok(new_id)
}

pub fn credit_note_from(conn: &Connection, id: i64) -> Result<i64> {
    let src: Document = db::get(conn, id)?;
    if src.doc_type != "invoice" || src.status == "draft" {
        bail_user!("Gutschriften können nur zu abgeschlossenen Rechnungen erstellt werden.");
    }
    let lines = lines_of(conn, id)?;
    let mut d = derived_doc(conn, &src, "credit_note", "standard")?;
    d.reference = src.number.clone();
    let new_id = create(conn, DocumentInput { doc: d, lines: clone_lines(&lines, false, true) })?;
    cfg::log(conn, "document", new_id, "converted", format!("Gutschrift zu Rechnung {}", src.number))?;
    Ok(new_id)
}

pub fn cancel(conn: &Connection, id: i64) -> Result<i64> {
    let mut src: Document = db::get(conn, id)?;
    if src.doc_type != "invoice" || src.status == "draft" {
        bail_user!("Nur abgeschlossene Rechnungen können storniert werden.");
    }
    if src.status == "cancelled" {
        bail_user!("Die Rechnung ist bereits storniert.");
    }
    if src.kind == "cancellation" {
        bail_user!("Eine Stornorechnung kann nicht storniert werden.");
    }
    let paid = db::scalar_i64(conn, "SELECT COUNT(*) FROM allocations WHERE document_id = ?1", &[v(id)])?;
    if paid > 0 {
        bail_user!("Der Rechnung sind Zahlungen zugeordnet. Bitte zuerst die Zahlungen entfernen oder eine Gutschrift erstellen.");
    }
    if src.kind == "partial" {
        let finals = db::scalar_i64(
            conn,
            "SELECT COUNT(*) FROM documents WHERE kind = 'final' AND status != 'draft' AND status != 'cancelled' AND base_id = ?1",
            &[v(src.base_id.unwrap_or(0))],
        )?;
        if finals > 0 {
            bail_user!("Zu dieser Abschlagsrechnung existiert bereits eine Schlussrechnung.");
        }
    }
    cfg::check_unlocked(conn, today())?;
    let lines = lines_of(conn, id)?;
    let mut d = derived_doc(conn, &src, "invoice", "cancellation")?;
    d.cancels_id = Some(id);
    d.source_id = None;
    d.base_id = None;
    d.reference = src.number.clone();
    d.due_date = None;
    d.payment_terms_days = None;
    d.service_date = src.service_date;
    d.service_date_end = src.service_date_end;
    let vars = [("Bezugsnummer", src.number.clone()), ("Bezugsdatum", fmt_date(src.date))];
    d.intro_text = util::fill_placeholders(&d.intro_text, &vars);
    let new_id = create(conn, DocumentInput { doc: d, lines: clone_lines(&lines, true, false) })?;
    // The cancellation mirrors an issued invoice; it must stay possible even if that one was incomplete.
    let storno = issue(conn, new_id, false)?;
    let mut st = storno.clone();
    st.status = "cancelled".into();
    db::update(conn, &st)?;
    src.status = "cancelled".into();
    src.updated_at = now_str();
    db::update(conn, &src)?;
    cfg::log(conn, "document", id, "cancelled", format!("Storniert durch {}", storno.number))?;
    cfg::log(conn, "document", new_id, "cancelled", format!("Storno zu Rechnung {}", src.number))?;
    posting::rebuild(conn, posting::Source::Document(id))?;
    posting::rebuild(conn, posting::Source::Document(new_id))?;
    Ok(new_id)
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct PartialRequest {
    /// percent | amount
    pub mode: String,
    #[ts(type = "number")]
    pub value: Decimal,
}

fn base_for_partials(conn: &Connection, id: i64) -> Result<Document> {
    let base: Document = db::get(conn, id)?;
    if base.doc_type != "quote" && base.doc_type != "order" && base.doc_type != "invoice" {
        bail_user!("Abschlags- und Schlussrechnungen basieren auf einem Angebot oder einer Auftragsbestätigung.");
    }
    if base.doc_type == "invoice" {
        bail_user!("Bitte die Abschlagsrechnung aus dem Angebot bzw. der Auftragsbestätigung erstellen.");
    }
    if base.status == "draft" {
        bail_user!("Bitte das Angebot / die Auftragsbestätigung zuerst abschließen.");
    }
    Ok(base)
}

pub fn create_partial(conn: &Connection, base_id: i64, req: PartialRequest) -> Result<i64> {
    let base = base_for_partials(conn, base_id)?;
    let mut base_lines = lines_of(conn, base_id)?;
    let totals = calc::calculate(&base, &mut base_lines);
    if req.value <= Decimal::ZERO {
        bail_user!("Bitte einen Abschlag größer 0 angeben.");
    }
    let existing = db::query::<Document>(
        conn,
        "WHERE base_id = ?1 AND kind = 'partial' AND status != 'cancelled'",
        &[v(base_id)],
    )?;
    let already: Decimal = existing.iter().map(|d| d.net_total).sum();
    let nr = existing.len() + 1;
    let total_net = totals.items_net;
    let (pct, target_net) = if req.mode == "amount" {
        let pct = if total_net.is_zero() { Decimal::ZERO } else { req.value / total_net * HUNDRED };
        (pct, req.value)
    } else {
        (req.value, r2(total_net * req.value / HUNDRED))
    };
    if already + target_net > total_net + Decimal::new(1, 2) {
        bail_user!(
            "Die Abschläge ({}) würden den Auftragswert ({}) übersteigen.",
            fmt_money(already + target_net),
            fmt_money(total_net)
        );
    }
    let weights: Vec<Decimal> = totals.item_groups.iter().map(|g| g.net).collect();
    let parts = calc::distribute(target_net, &weights);
    let base_label = format!("{} {} vom {}", type_label(&base.doc_type, &base.kind), base.number, fmt_date(base.date));
    let mut lines = Vec::new();
    for (g, net) in totals.item_groups.iter().zip(parts) {
        let rate_txt = if totals.item_groups.len() > 1 { format!(" (Leistungen zu {})", util::fmt_percent(g.rate)) } else { String::new() };
        lines.push(DocumentLine {
            line_type: "item".into(),
            name: format!("{nr}. Abschlag ({}){rate_txt}", util::fmt_percent(r2(pct))),
            description: format!("gemäß {base_label}\nAuftragswert netto: {}", fmt_money(g.net)),
            quantity: Decimal::ONE,
            unit: "Pauschal".into(),
            unit_price: if base.prices_gross { r2(net * (HUNDRED + g.rate) / HUNDRED) } else { net },
            tax_rate: g.rate,
            ..Default::default()
        });
    }
    let mut d = derived_doc(conn, &base, "invoice", "partial")?;
    d.base_id = Some(base_id);
    d.partial_mode = req.mode.clone();
    d.partial_value = req.value;
    d.reference = if base.reference.is_empty() { base.number.clone() } else { base.reference.clone() };
    let id = create(conn, DocumentInput { doc: d, lines })?;
    cfg::log(conn, "document", id, "created", format!("{nr}. Abschlagsrechnung zu {}", base.number))?;
    Ok(id)
}

pub fn create_final(conn: &Connection, base_id: i64) -> Result<i64> {
    let base = base_for_partials(conn, base_id)?;
    let existing_final = db::scalar_i64(
        conn,
        "SELECT COUNT(*) FROM documents WHERE base_id = ?1 AND kind = 'final' AND status != 'cancelled'",
        &[v(base_id)],
    )?;
    if existing_final > 0 {
        bail_user!("Zu diesem Auftrag existiert bereits eine Schlussrechnung.");
    }
    let base_lines = lines_of(conn, base_id)?;
    let mut lines = clone_lines(&base_lines, false, true);
    let partials = db::query::<Document>(
        conn,
        "WHERE base_id = ?1 AND kind = 'partial' AND status NOT IN ('draft','cancelled') ORDER BY date, id",
        &[v(base_id)],
    )?;
    for p in &partials {
        let mut pl = lines_of(conn, p.id)?;
        let t = calc::calculate(p, &mut pl);
        for g in &t.groups {
            lines.push(DocumentLine {
                line_type: "deduction".into(),
                name: format!("abzüglich Abschlagsrechnung {} vom {}", p.number, fmt_date(p.date)),
                quantity: Decimal::ONE,
                tax_rate: g.rate,
                net_amount: -g.net,
                tax_amount: -g.tax,
                ref_document_id: Some(p.id),
                ..Default::default()
            });
        }
    }
    let mut d = derived_doc(conn, &base, "invoice", "final")?;
    d.base_id = Some(base_id);
    d.reference = if base.reference.is_empty() { base.number.clone() } else { base.reference.clone() };
    let id = create(conn, DocumentInput { doc: d, lines })?;
    cfg::log(conn, "document", id, "created", format!("Schlussrechnung zu {} ({} Abschläge)", base.number, partials.len()))?;
    Ok(id)
}

/// Recomputes `paid_total`, `status` and `paid_at` of an invoice / credit note.
pub fn recompute_paid(conn: &Connection, id: i64) -> Result<()> {
    let Some(mut doc) = db::find::<Document>(conn, id)? else { return Ok(()) };
    if !doc.is_invoice_like() || doc.status == "draft" || doc.status == "cancelled" {
        return Ok(());
    }
    let allocs = db::query::<Allocation>(conn, "WHERE document_id = ?1", &[v(id)])?;
    let sum: Decimal = allocs.iter().map(|a| a.amount).sum();
    doc.paid_total = sum * doc.sign();
    // A total of 0 (e.g. a Schlussrechnung covered by its Abschläge) is settled right away; a
    // negative total is settled by a refund.
    let settled = doc.remaining() <= Decimal::ZERO;
    doc.status = if settled {
        "paid".into()
    } else if !doc.paid_total.is_zero() && doc.paid_total.signum() == doc.gross_total.signum() {
        "partial".into()
    } else {
        "open".into()
    };
    doc.paid_at = if settled { allocs.iter().map(|a| a.date).max().or(Some(doc.date)) } else { None };
    db::update(conn, &doc)?;
    Ok(())
}

/// Text placeholders for a document.
pub fn placeholders(s: &Settings, doc: &Document, contact: Option<&Contact>) -> Vec<(&'static str, String)> {
    let anrede = match contact {
        Some(c) if c.kind == "person" || c.company.is_empty() => util::salutation_line(&c.salutation, &c.title, &c.last_name),
        Some(c) => match c.primary_person() {
            Some(p) => util::salutation_line(&p.salutation, &p.title, &p.last_name),
            None => "Sehr geehrte Damen und Herren,".into(),
        },
        None => "Sehr geehrte Damen und Herren,".into(),
    };
    let skonto = if doc.cash_discount_percent > Decimal::ZERO && doc.cash_discount_days > 0 {
        let until = doc.date + Duration::days(doc.cash_discount_days);
        format!(
            "Bei Zahlung bis zum {} gewähren wir {} Skonto ({}).",
            fmt_date(until),
            util::fmt_percent(doc.cash_discount_percent),
            fmt_money(r2(doc.gross_total * doc.cash_discount_percent / HUNDRED))
        )
    } else {
        String::new()
    };
    vec![
        ("Anrede", anrede),
        ("Kunde", contact.map(|c| c.display_name()).unwrap_or_else(|| doc.address.lines().next().unwrap_or_default().to_string())),
        ("Ansprechpartner", doc.contact_person.clone()),
        ("Nummer", if doc.number.is_empty() { "(Entwurf)".into() } else { doc.number.clone() }),
        ("Datum", fmt_date(doc.date)),
        ("Betrag", fmt_money(doc.gross_total)),
        ("Fällig am", util::fmt_date_opt(doc.due_date)),
        ("Zahlungsziel", doc.payment_terms_days.map(|d| format!("{d} Tage")).unwrap_or_default()),
        ("Gültig bis", util::fmt_date_opt(doc.valid_until)),
        ("Skonto", skonto),
        ("Firma", s.company.name.clone()),
        ("Referenz", doc.reference.clone()),
        ("Kundennummer", contact.map(|c| c.number.clone()).unwrap_or_default()),
    ]
    .into_iter()
    .chain(period_placeholders(doc))
    .collect()
}

/// Placeholders of the service period (start of the period, else the document date), e.g. for the
/// positions of an Abo-Rechnung: "Wartung {{Monat}}" becomes "Wartung März 2026".
pub fn period_placeholders(doc: &Document) -> Vec<(&'static str, String)> {
    let start = doc.service_date.unwrap_or(doc.date);
    let period = match (doc.service_date, doc.service_date_end) {
        (Some(a), Some(b)) if a != b => format!("{} – {}", fmt_date(a), fmt_date(b)),
        _ => fmt_date(start),
    };
    vec![
        ("Leistungszeitraum", period),
        ("Monat", format!("{} {}", util::month_name(start.month()), start.year())),
        ("Quartal", format!("{}. Quartal {}", start.month0() / 3 + 1, start.year())),
        ("Jahr", start.year().to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PaymentAccount;
    use crate::services::finance::{self, PaymentRequest};
    use crate::util::ymd;
    use rust_decimal_macros::dec;

    fn setup() -> (Connection, i64) {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        cfg::seed_test_company(&conn);
        let bank = finance::save_account(&conn, PaymentAccount { name: "Bank".into(), opening_date: ymd(2020, 1, 1), ..Default::default() }).unwrap();
        (conn, bank)
    }

    fn item(price: Decimal) -> DocumentLine {
        DocumentLine { line_type: "item".into(), name: "Leistung".into(), quantity: dec!(1), unit_price: price, tax_rate: dec!(19), ..Default::default() }
    }

    /// Finalized order with one fully paid Abschlag of `percent` %.
    fn order_with_paid_partial(conn: &Connection, bank: i64, percent: Decimal) -> i64 {
        let doc = Document { doc_type: "order".into(), address: "Kunde GmbH\nWeg 1\n12345 Stadt".into(), tax_mode: "standard".into(), ..Default::default() };
        let order = create(conn, DocumentInput { doc, lines: vec![item(dec!(1000))] }).unwrap();
        finalize(conn, order).unwrap();
        let partial = create_partial(conn, order, PartialRequest { mode: "percent".into(), value: percent }).unwrap();
        let p = finalize(conn, partial).unwrap();
        finance::pay_document(conn, partial, PaymentRequest { account_id: Some(bank), amount: p.gross_total, ..Default::default() }).unwrap();
        order
    }

    #[test]
    fn invoices_need_the_mandatory_data() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        let invoice = |conn: &Connection, address: &str, price: Decimal, name: &str| {
            let doc = Document { doc_type: "invoice".into(), address: address.into(), tax_mode: "standard".into(), date: ymd(2026, 3, 1), ..Default::default() };
            let line = DocumentLine { name: name.into(), ..item(price) };
            create(conn, DocumentInput { doc, lines: vec![line] }).unwrap()
        };
        let id = invoice(&conn, "Kunde GmbH", dec!(1000), "Leistung");
        let err = finalize(&conn, id).unwrap_err().to_string();
        assert!(err.contains("Anschrift Ihres Unternehmens") && err.contains("Steuernummer") && err.contains("Anschrift des Empfängers"), "{err}");
        cfg::seed_test_company(&conn);
        assert!(finalize(&conn, invoice(&conn, "Kunde GmbH\nWeg 1\n12345 Stadt", dec!(1000), "")).unwrap_err().to_string().contains("Bezeichnung"));
        assert!(finalize(&conn, invoice(&conn, "Kunde GmbH\nWeg 1\n12345 Stadt", dec!(1000), "Leistung")).is_ok());
        // Kleinbetragsrechnung (≤ 250 € gross): no recipient address and tax number needed.
        let mut s = cfg::load(&conn).unwrap();
        s.company.tax_number.clear();
        s.company.vat_id.clear();
        cfg::save(&conn, &s).unwrap();
        assert!(finalize(&conn, invoice(&conn, "Barverkauf", dec!(200), "Leistung")).is_ok());
    }

    #[test]
    fn final_invoice_covered_by_partials_is_paid() {
        let (conn, bank) = setup();
        let order = order_with_paid_partial(&conn, bank, dec!(100));
        let fin = finalize(&conn, create_final(&conn, order).unwrap()).unwrap();
        let fin: Document = db::get(&conn, fin.id).unwrap();
        assert_eq!((fin.gross_total, fin.status.as_str(), fin.paid_at.is_some()), (dec!(0), "paid", true));
        assert!(list(&conn, &DocumentFilter { status: "unpaid".into(), ..Default::default() }).unwrap().is_empty());
    }

    #[test]
    fn negative_final_invoice_is_settled_by_refund() {
        let (conn, bank) = setup();
        let order = order_with_paid_partial(&conn, bank, dec!(100));
        // The order is reduced after the Abschlag: the Schlussrechnung ends below zero.
        let mut input = DocumentInput { doc: db::get(&conn, order).unwrap(), lines: vec![item(dec!(500))] };
        input.doc.id = order;
        update(&conn, order, input).unwrap();
        let fin = finalize(&conn, create_final(&conn, order).unwrap()).unwrap().id;
        let doc: Document = db::get(&conn, fin).unwrap();
        assert_eq!((doc.gross_total, doc.status.as_str(), doc.remaining(), doc.money_sign()), (dec!(-595), "open", dec!(595), dec!(-1)));
        assert_eq!(list(&conn, &DocumentFilter { status: "unpaid".into(), ..Default::default() }).unwrap().len(), 1);

        finance::pay_document(&conn, fin, PaymentRequest { account_id: Some(bank), amount: dec!(595), ..Default::default() }).unwrap();
        let doc: Document = db::get(&conn, fin).unwrap();
        assert_eq!((doc.status.as_str(), doc.remaining()), ("paid", dec!(0)));
        let money: Decimal = db::query::<crate::models::Transaction>(&conn, "", &[]).unwrap().iter().map(|t| t.amount).sum();
        assert_eq!(money, dec!(1190) - dec!(595), "refund paid out");
    }
}
