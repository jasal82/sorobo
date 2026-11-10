// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Builds the JSON data consumed by the Typst templates. All values are
//! pre-formatted (German number and date formats) so that the templates stay
//! purely presentational.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{Value, json};

use crate::db;
use crate::models::*;
use crate::services::calc::{self, Totals};
use crate::services::documents::{self, type_label};
use crate::services::settings as cfg;
use crate::util::{self, fmt_date, fmt_date_opt, fmt_money, fmt_percent, fmt_price, fmt_qty};

use super::girocode;

/// Everything needed to render one document: template JSON plus binary files.
pub struct RenderJob {
    pub source: &'static str,
    pub files: HashMap<String, Vec<u8>>,
}

impl RenderJob {
    fn new(source: &'static str, data: Value) -> Self {
        let mut files = HashMap::new();
        files.insert("data.json".to_string(), serde_json::to_vec(&data).unwrap_or_default());
        Self { source, files }
    }

    /// Embeds a file into the PDF (e.g. factur-x.xml for ZUGFeRD).
    pub fn attach(&mut self, name: &str, bytes: Vec<u8>) {
        if let Some(raw) = self.files.get("data.json")
            && let Ok(mut data) = serde_json::from_slice::<Value>(raw)
        {
            data["doc"]["attachment"] = Value::String(name.to_string());
            self.files.insert("data.json".into(), serde_json::to_vec(&data).unwrap_or_default());
        }
        self.files.insert(name.to_string(), bytes);
    }
}

fn file_bytes(conn: &Connection, data_dir: &Path, id: Option<i64>) -> Result<Option<(String, Vec<u8>)>> {
    let Some(id) = id else { return Ok(None) };
    let Some(meta) = db::find::<FileMeta>(conn, id)? else { return Ok(None) };
    let path = data_dir.join("files").join(&meta.uuid);
    let Ok(bytes) = std::fs::read(&path) else { return Ok(None) };
    let ext = match meta.mime.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        "application/pdf" => "pdf",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => Path::new(&meta.filename).extension().and_then(|e| e.to_str()).unwrap_or("png"),
    };
    Ok(Some((ext.to_string(), bytes)))
}

pub fn layout_for(conn: &Connection, s: &Settings, layout_id: Option<i64>) -> Result<LayoutConfig> {
    let by_id = match layout_id {
        Some(id) => db::find::<Layout>(conn, id)?,
        None => None,
    };
    let by_default = match s.docs.default_layout_id {
        Some(id) => db::find::<Layout>(conn, id)?,
        None => None,
    };
    let any_default = db::query_one::<Layout>(conn, "ORDER BY is_default DESC, id", &[])?;
    Ok(by_id.or(by_default).or(any_default).map(|l| l.config).unwrap_or_default())
}

fn company_json(s: &Settings, layout: &LayoutConfig) -> Value {
    let c = &s.company;
    let city = format!("{} {}", c.zip, c.city).trim().to_string();
    let sender = if layout.sender_line.trim().is_empty() {
        [c.name.as_str(), c.street.as_str(), city.as_str()].iter().filter(|x| !x.trim().is_empty()).cloned().collect::<Vec<_>>().join(" · ")
    } else {
        layout.sender_line.clone()
    };
    let mut address = vec![c.name.clone()];
    address.push(c.street.clone());
    address.push(city.clone());
    address.retain(|x| !x.trim().is_empty());
    let mut contact = Vec::new();
    if !c.phone.is_empty() {
        contact.push(format!("Tel. {}", c.phone));
    }
    if !c.email.is_empty() {
        contact.push(c.email.clone());
    }
    if !c.website.is_empty() {
        contact.push(c.website.clone());
    }
    let mut tax = Vec::new();
    if !c.vat_id.is_empty() {
        tax.push(format!("USt-IdNr.: {}", c.vat_id));
    }
    if !c.tax_number.is_empty() {
        tax.push(format!("Steuernummer: {}", c.tax_number));
    }
    if !c.register_number.is_empty() {
        tax.push(format!("{} {}", c.register_court, c.register_number).trim().to_string());
    }
    if !c.owner.is_empty() {
        let role = match c.legal_form.as_str() {
            "GmbH" | "UG (haftungsbeschränkt)" | "GmbH & Co. KG" => "Geschäftsführung",
            "AG" | "e.V." => "Vorstand",
            "GbR" | "OHG" | "KG" => "Gesellschafter",
            _ => "Inhaber",
        };
        tax.push(format!("{role}: {}", c.owner.replace(' ', "\u{a0}")));
    }
    let mut bank = Vec::new();
    if !c.bank_name.is_empty() {
        bank.push(c.bank_name.clone());
    }
    if !c.iban.is_empty() {
        bank.push(format!("IBAN: {}", util::format_iban(&c.iban).replace(' ', "\u{a0}")));
    }
    if !c.bic.is_empty() {
        bank.push(format!("BIC: {}", c.bic));
    }
    let footer: Vec<Vec<String>> = if layout.footer_mode == "custom" && !layout.footer_custom.is_empty() {
        layout.footer_custom.iter().map(|col| col.lines().map(|l| l.to_string()).collect()).collect()
    } else {
        vec![address.clone(), contact.clone(), bank.clone(), tax.clone()].into_iter().filter(|c| !c.is_empty()).collect()
    };
    json!({
        "name": c.name,
        "sender_line": sender,
        "address": address,
        "contact": contact,
        "footer": footer,
    })
}

fn layout_json(layout: &LayoutConfig, files: &mut HashMap<String, Vec<u8>>, conn: &Connection, data_dir: &Path, s: &Settings) -> Result<Value> {
    let mut l = serde_json::to_value(layout)?;
    let logo_id = layout.logo_file_id.or(s.company.logo_file_id);
    let mut logo = Value::Null;
    if let Some((ext, bytes)) = file_bytes(conn, data_dir, logo_id)? {
        let name = format!("logo.{ext}");
        files.insert(name.clone(), bytes);
        logo = Value::String(name);
    }
    let mut lh_first = Value::Null;
    if let Some((ext, bytes)) = file_bytes(conn, data_dir, layout.letterhead_first_id)? {
        let name = format!("letterhead1.{ext}");
        files.insert(name.clone(), bytes);
        lh_first = Value::String(name);
    }
    let mut lh_other = Value::Null;
    if let Some((ext, bytes)) = file_bytes(conn, data_dir, layout.letterhead_other_id)? {
        let name = format!("letterhead2.{ext}");
        files.insert(name.clone(), bytes);
        lh_other = Value::String(name);
    }
    l["logo"] = logo;
    l["letterhead_first"] = lh_first;
    l["letterhead_other"] = lh_other;
    Ok(l)
}

pub fn tax_note(doc: &Document, contact: Option<&Contact>, s: &Settings) -> Vec<String> {
    let mut notes = Vec::new();
    let buyer_vat = contact.map(|c| c.vat_id.clone()).unwrap_or_default();
    let vat_line = || {
        let mut parts = Vec::new();
        if !s.company.vat_id.is_empty() {
            parts.push(format!("Unsere USt-IdNr.: {}", s.company.vat_id));
        }
        if !buyer_vat.is_empty() {
            parts.push(format!("Ihre USt-IdNr.: {buyer_vat}"));
        }
        parts.join(" · ")
    };
    match doc.tax_mode.as_str() {
        "small_business" => notes.push("Gemäß § 19 UStG wird keine Umsatzsteuer berechnet (Kleinunternehmerregelung).".into()),
        "reverse_charge" => notes.push("Steuerschuldnerschaft des Leistungsempfängers gemäß § 13b UStG.".into()),
        "eu_service" => {
            notes.push("Steuerschuldnerschaft des Leistungsempfängers (Reverse Charge) – Art. 196 MwStSystRL, § 13b UStG.".into());
            notes.push(vat_line());
        }
        "eu_delivery" => {
            notes.push("Steuerfreie innergemeinschaftliche Lieferung gemäß § 4 Nr. 1b i. V. m. § 6a UStG.".into());
            notes.push(vat_line());
        }
        "export" => notes.push("Steuerfreie Ausfuhrlieferung gemäß § 4 Nr. 1a i. V. m. § 6 UStG.".into()),
        "third_country" => notes.push("Nicht im Inland steuerbare Leistung (Leistungsort im Drittland).".into()),
        _ => {}
    }
    notes.retain(|n| !n.is_empty());
    notes
}

fn service_period(doc: &Document) -> String {
    match (doc.service_date, doc.service_date_end) {
        (Some(a), Some(b)) if a != b => format!("{} – {}", fmt_date(a), fmt_date(b)),
        (Some(a), _) => fmt_date(a),
        _ => String::new(),
    }
}

#[derive(Serialize)]
struct Row {
    #[serde(rename = "type")]
    kind: String,
    pos: String,
    number: String,
    name: String,
    description: String,
    qty: String,
    unit: String,
    price: String,
    discount: String,
    tax: String,
    total: String,
}

pub fn document_job(
    conn: &Connection,
    data_dir: &Path,
    doc: &Document,
    lines: &[DocumentLine],
    layout_override: Option<LayoutConfig>,
) -> Result<RenderJob> {
    let s = cfg::load(conn)?;
    let mut lines = lines.to_vec();
    let totals = calc::calculate(doc, &mut lines);
    let contact = doc.contact_id.and_then(|c| db::find::<Contact>(conn, c).ok().flatten());
    let layout = match layout_override {
        Some(l) => l,
        None => layout_for(conn, &s, doc.layout_id)?,
    };
    let mut files = HashMap::new();
    let layout_v = layout_json(&layout, &mut files, conn, data_dir, &s)?;
    let is_delivery = doc.doc_type == "delivery";
    let label = type_label(&doc.doc_type, &doc.kind);
    let number_label = match doc.doc_type.as_str() {
        "invoice" => "Rechnungsnr.",
        "credit_note" => "Gutschriftsnr.",
        "quote" => "Angebotsnr.",
        "order" => "Auftragsnr.",
        _ => "Lieferscheinnr.",
    };
    let number = if doc.number.is_empty() { "Entwurf".to_string() } else { doc.number.clone() };
    // Placeholders also work in the title and the positions (e.g. "Wartung {{Monat}}").
    let mut vars = documents::placeholders(&s, doc, contact.as_ref());
    vars.extend(crate::services::debit::placeholders(conn, &s, doc)?);
    let fill = |t: &str| util::fill_placeholders(t, &vars);
    let title = fill(&doc.title);
    let mut info: Vec<(String, String)> = vec![(number_label.into(), number.clone()), ("Datum".into(), fmt_date(doc.date))];
    if let Some(c) = &contact
        && !c.number.is_empty()
    {
        info.push(("Kundennr.".into(), c.number.clone()));
    }
    let sp = service_period(doc);
    if !sp.is_empty() && doc.doc_type != "quote" {
        info.push((if doc.service_date_end.is_some() { "Leistungszeitraum".into() } else { "Leistungsdatum".into() }, sp));
    }
    if doc.doc_type == "quote" && doc.valid_until.is_some() {
        info.push(("Gültig bis".into(), fmt_date_opt(doc.valid_until)));
    }
    if doc.is_invoice_like() && doc.kind != "cancellation" && doc.due_date.is_some() {
        info.push((if doc.payment_method == "debit" { "Einzug am".into() } else { "Fällig am".into() }, fmt_date_opt(doc.due_date)));
    }
    if !doc.reference.is_empty() {
        info.push(("Ihre Referenz".into(), doc.reference.clone()));
    }
    if !doc.buyer_reference.is_empty() {
        info.push(("Leitweg-ID".into(), doc.buyer_reference.clone()));
    }
    if !doc.contact_person.is_empty() && layout.show_company_block {
        info.push(("Ansprechpartner".into(), doc.contact_person.clone()));
    }
    if !s.company.contact_name.is_empty() {
        info.push(("Ihr Kontakt".into(), s.company.contact_name.clone()));
    }

    let show_discount = lines.iter().any(|l| l.line_type == "item" && !l.discount_percent.is_zero());
    let mut rows = Vec::new();
    let mut pos = 0;
    for l in &lines {
        match l.line_type.as_str() {
            "item" => {
                pos += 1;
                let total = if doc.prices_gross { l.gross_amount } else { l.net_amount };
                rows.push(Row {
                    kind: "item".into(),
                    pos: pos.to_string(),
                    number: l.article_number.clone(),
                    name: fill(&l.name),
                    description: fill(&l.description),
                    qty: fmt_qty(l.quantity),
                    unit: l.unit.clone(),
                    price: fmt_price(l.unit_price),
                    discount: if l.discount_percent.is_zero() { String::new() } else { fmt_percent(l.discount_percent) },
                    tax: fmt_percent(calc::effective_rate(doc, l)),
                    total: fmt_money(total),
                });
            }
            "text" => rows.push(Row {
                kind: "text".into(),
                pos: String::new(),
                number: String::new(),
                name: fill(&l.name),
                description: fill(&l.description),
                qty: String::new(),
                unit: String::new(),
                price: String::new(),
                discount: String::new(),
                tax: String::new(),
                total: String::new(),
            }),
            "subtotal" => rows.push(Row {
                kind: "subtotal".into(),
                pos: String::new(),
                number: String::new(),
                name: if l.name.is_empty() { "Zwischensumme".into() } else { l.name.clone() },
                description: String::new(),
                qty: String::new(),
                unit: String::new(),
                price: String::new(),
                discount: String::new(),
                tax: String::new(),
                total: fmt_money(if doc.prices_gross { l.gross_amount } else { l.net_amount }),
            }),
            "pagebreak" => rows.push(Row {
                kind: "pagebreak".into(),
                pos: String::new(),
                number: String::new(),
                name: String::new(),
                description: String::new(),
                qty: String::new(),
                unit: String::new(),
                price: String::new(),
                discount: String::new(),
                tax: String::new(),
                total: String::new(),
            }),
            _ => {}
        }
    }

    let totals_rows = if is_delivery { vec![] } else { totals_json(doc, &totals) };
    let intro = util::fill_placeholders(&doc.intro_text, &vars);
    let closing = util::fill_placeholders(&doc.closing_text, &vars);
    let mut notes = tax_note(doc, contact.as_ref(), &s);
    if let Some(p) = crate::services::debit::prenotification(conn, &s, doc, &closing)? {
        notes.push(p);
    }
    if doc.is_invoice_like() && doc.cash_discount_percent > Decimal::ZERO && doc.cash_discount_days > 0 && doc.kind != "cancellation" {
        if let Some((_, sk)) = vars.iter().find(|(k, _)| *k == "Skonto")
            && !sk.is_empty()
            && !closing.contains(sk.as_str())
        {
            notes.push(sk.clone());
        }
    }

    // GiroCode (EPC QR) for the amount still to be paid.
    let giro_amount = if doc.status == "draft" { doc.gross_total } else { doc.open_amount() };
    let mut giro = Value::Null;
    if layout.show_girocode
        && doc.doc_type == "invoice"
        && doc.kind != "cancellation"
        && doc.payment_method != "debit"
        && giro_amount > Decimal::ZERO
        && !s.company.iban.is_empty()
        && let Some(payload) = girocode::payload(&s.company.name, &s.company.iban, &s.company.bic, giro_amount, &number)
        && let Some(svg) = girocode::svg(&payload)
    {
        files.insert("girocode.svg".into(), svg);
        giro = json!({ "file": "girocode.svg", "amount": fmt_money(giro_amount), "iban": util::format_iban(&s.company.iban), "reference": number });
    }

    let heading = if doc.number.is_empty() { label.to_string() } else { format!("{title} {}", doc.number) };
    let data = json!({
        "layout": layout_v,
        "company": company_json(&s, &layout),
        "doc": {
            "kind": doc.doc_type,
            "label": label,
            "title": title,
            "heading": heading,
            "number": number,
            "is_draft": doc.status == "draft",
            "recipient": doc.address.lines().collect::<Vec<_>>(),
            "info": info,
            "intro": intro,
            "closing": closing,
            // Not for quotes that were already accepted or rejected.
            "signature": layout.show_signature && doc.doc_type == "quote" && matches!(doc.status.as_str(), "draft" | "open"),
            "columns": {
                "pos": layout.show_position,
                "number": layout.show_article_number && lines.iter().any(|l| !l.article_number.is_empty()),
                "unit": layout.show_unit_column,
                "discount": show_discount && !is_delivery,
                "tax": layout.show_tax_column && doc.tax_mode == "standard" && !is_delivery,
                "prices": !is_delivery,
            },
            "price_label": if doc.prices_gross { "Einzelpreis (brutto)" } else { "Einzelpreis" },
            "total_label": if doc.prices_gross { "Gesamt (brutto)" } else { "Gesamt" },
            "rows": rows,
            "totals": totals_rows,
            "notes": notes,
            "girocode": giro,
            "custom_table": Value::Null,
        }
    });
    let mut job = RenderJob::new(super::DOCUMENT_TEMPLATE, data);
    job.files.extend(files);
    Ok(job)
}

fn totals_json(doc: &Document, t: &Totals) -> Vec<Value> {
    let row = |label: String, value: String, style: &str| json!({ "label": label, "value": value, "style": style });
    let mut rows = Vec::new();
    let standard = doc.tax_mode == "standard";
    if doc.kind == "final" && !t.deductions.is_empty() {
        rows.push(row("Gesamtleistung netto".into(), fmt_money(t.items_net), "normal"));
        for g in t.item_groups.iter().filter(|g| standard && !g.rate.is_zero()) {
            rows.push(row(format!("zzgl. {} USt auf {}", fmt_percent(g.rate), fmt_money(g.net)), fmt_money(g.tax), "normal"));
        }
        rows.push(row("Gesamtleistung brutto".into(), fmt_money(t.items_gross), "subtotal"));
        for d in &t.deductions {
            let detail = if standard { format!(" (netto {}, USt {})", fmt_money(-d.net), fmt_money(-d.tax)) } else { String::new() };
            rows.push(row(format!("{}{}", d.label, detail), fmt_money(d.gross), "deduction"));
        }
        rows.push(row(if doc.gross_total >= Decimal::ZERO { "Restbetrag".into() } else { "Guthaben".into() }, fmt_money(t.gross), "total"));
        if standard {
            for g in t.groups.iter().filter(|g| !g.rate.is_zero()) {
                rows.push(row(format!("darin enthaltene USt {} auf {}", fmt_percent(g.rate), fmt_money(g.net)), fmt_money(g.tax), "muted"));
            }
        }
        return rows;
    }
    if !standard {
        rows.push(row("Gesamtbetrag".into(), fmt_money(t.gross), "total"));
        return rows;
    }
    if doc.prices_gross {
        rows.push(row("Gesamtbetrag (brutto)".into(), fmt_money(t.gross), "total"));
        for g in t.groups.iter().filter(|g| !g.rate.is_zero()) {
            rows.push(row(format!("darin enthaltene USt {} auf {}", fmt_percent(g.rate), fmt_money(g.net)), fmt_money(g.tax), "muted"));
        }
        rows.push(row("Nettobetrag".into(), fmt_money(t.net), "muted"));
    } else {
        rows.push(row("Nettobetrag".into(), fmt_money(t.net), "normal"));
        for g in &t.groups {
            if g.rate.is_zero() {
                continue;
            }
            rows.push(row(format!("zzgl. USt {} auf {}", fmt_percent(g.rate), fmt_money(g.net)), fmt_money(g.tax), "normal"));
        }
        rows.push(row("Gesamtbetrag".into(), fmt_money(t.gross), "total"));
    }
    rows
}

/// Dunning letter.
pub fn dunning_job(conn: &Connection, data_dir: &Path, dn: &Dunning) -> Result<RenderJob> {
    let s = cfg::load(conn)?;
    let doc: Document = db::get(conn, dn.document_id)?;
    let contact = doc.contact_id.and_then(|c| db::find::<Contact>(conn, c).ok().flatten());
    let layout = layout_for(conn, &s, doc.layout_id)?;
    let mut files = HashMap::new();
    let layout_v = layout_json(&layout, &mut files, conn, data_dir, &s)?;
    let table_rows = vec![vec![
        doc.number.clone(),
        fmt_date(doc.date),
        fmt_date_opt(doc.due_date),
        fmt_money(doc.gross_total),
        fmt_money(doc.paid_total),
        fmt_money(dn.open_amount),
    ]];
    let mut totals = vec![json!({"label": "Offener Rechnungsbetrag", "value": fmt_money(dn.open_amount), "style": "normal"})];
    if !dn.fee.is_zero() {
        totals.push(json!({"label": "Mahngebühren", "value": fmt_money(dn.fee), "style": "normal"}));
    }
    if !dn.interest.is_zero() {
        totals.push(json!({"label": "Verzugszinsen", "value": fmt_money(dn.interest), "style": "normal"}));
    }
    totals.push(json!({"label": "Zu zahlender Gesamtbetrag", "value": fmt_money(dn.open_amount + dn.fee + dn.interest), "style": "total"}));
    let mut info: Vec<(String, String)> = vec![("Datum".into(), fmt_date(dn.date)), ("Rechnungsnr.".into(), doc.number.clone())];
    if let Some(c) = &contact
        && !c.number.is_empty()
    {
        info.push(("Kundennr.".into(), c.number.clone()));
    }
    info.push(("Zahlbar bis".into(), fmt_date(dn.due_date)));
    let total_due = dn.open_amount + dn.fee + dn.interest;
    let mut giro = Value::Null;
    if layout.show_girocode
        && !s.company.iban.is_empty()
        && let Some(payload) = girocode::payload(&s.company.name, &s.company.iban, &s.company.bic, total_due, &doc.number)
        && let Some(svg) = girocode::svg(&payload)
    {
        files.insert("girocode.svg".into(), svg);
        giro = json!({ "file": "girocode.svg", "amount": fmt_money(total_due), "iban": util::format_iban(&s.company.iban), "reference": doc.number });
    }
    // The letter text is split so that the invoice table sits between the body
    // and the closing greeting.
    let (intro, closing) = match dn.text.find("\n\nMit freundlichen") {
        Some(i) => (dn.text[..i].to_string(), dn.text[i + 2..].to_string()),
        None => (dn.text.clone(), String::new()),
    };
    let data = json!({
        "layout": layout_v,
        "company": company_json(&s, &layout),
        "doc": {
            "kind": "dunning",
            "label": dn.title,
            "title": dn.title,
            "heading": dn.title,
            "number": doc.number,
            "is_draft": false,
            "recipient": doc.address.lines().collect::<Vec<_>>(),
            "info": info,
            "intro": intro,
            "closing": closing,
            "columns": { "pos": false, "number": false, "unit": false, "discount": false, "tax": false, "prices": true },
            "price_label": "",
            "total_label": "",
            "rows": [],
            "totals": totals,
            "notes": [],
            "girocode": giro,
            "custom_table": {
                "columns": ["Rechnung", "Datum", "Fällig am", "Betrag", "Bezahlt", "Offen"],
                "align": ["left", "left", "left", "right", "right", "right"],
                "rows": table_rows,
            },
        }
    });
    let mut job = RenderJob::new(super::DOCUMENT_TEMPLATE, data);
    job.files.extend(files);
    Ok(job)
}

/// Form of a SEPA-Lastschriftmandat for the customer to sign, with the texts prescribed by the
/// SEPA rulebook for Basis- and Firmenlastschrift.
pub fn mandate_job(conn: &Connection, data_dir: &Path, m: &Mandate) -> Result<RenderJob> {
    let s = cfg::load(conn)?;
    let contact: Contact = db::get(conn, m.contact_id)?;
    let layout = layout_for(conn, &s, None)?;
    let mut files = HashMap::new();
    let layout_v = layout_json(&layout, &mut files, conn, data_dir, &s)?;
    let c = &s.company;
    let b2b = m.scheme == "B2B";
    let title = if b2b { "SEPA-Firmenlastschrift-Mandat" } else { "SEPA-Lastschriftmandat" };
    let mut text = format!(
        "Ich ermächtige / Wir ermächtigen {name}, Zahlungen von meinem / unserem Konto mittels Lastschrift einzuziehen. Zugleich weise ich mein / weisen wir unser Kreditinstitut an, die von {name} auf mein / unser Konto gezogenen Lastschriften einzulösen.\n\n",
        name = c.name
    );
    text.push_str(if b2b {
        "Hinweis: Dieses Lastschriftmandat dient nur dem Einzug von Lastschriften, die auf Konten von Unternehmen gezogen sind. Ich bin / Wir sind nicht berechtigt, nach der erfolgten Einlösung eine Erstattung des belasteten Betrages zu verlangen. Ich bin / Wir sind berechtigt, mein / unser Kreditinstitut bis zum Fälligkeitstag anzuweisen, Lastschriften nicht einzulösen."
    } else {
        "Hinweis: Ich kann / Wir können innerhalb von acht Wochen, beginnend mit dem Belastungsdatum, die Erstattung des belasteten Betrages verlangen. Es gelten dabei die mit meinem / unserem Kreditinstitut vereinbarten Bedingungen."
    });
    let days = s.debit.prenotification_days;
    if (1..14).contains(&days) {
        text.push_str(&format!("\n\nDie Frist für die Vorabankündigung (Pre-Notification) der Lastschriften wird auf {days} {} verkürzt.", if days == 1 { "Tag" } else { "Tage" }));
    }
    let creditor = [c.name.as_str(), c.street.as_str(), format!("{} {}", c.zip, c.city).trim()].iter().filter(|x| !x.trim().is_empty()).cloned().collect::<Vec<_>>().join(", ");
    let rows = vec![
        vec!["Zahlungsempfänger".to_string(), creditor],
        vec!["Gläubiger-Identifikationsnummer".into(), util::normalize_iban(&s.debit.creditor_id)],
        vec!["Mandatsreferenz".into(), m.reference.clone()],
        vec!["Zahlungsart".into(), if m.recurring { "Wiederkehrende Zahlung".into() } else { "Einmalige Zahlung".into() }],
        vec!["Kontoinhaber".into(), if m.account_holder.is_empty() { contact.display_name() } else { m.account_holder.clone() }],
        vec!["IBAN".into(), util::format_iban(&m.iban)],
        vec!["BIC".into(), if m.bic.is_empty() { "–".into() } else { m.bic.clone() }],
    ];
    let mut info: Vec<(String, String)> = vec![("Mandatsreferenz".into(), m.reference.clone())];
    if !contact.number.is_empty() {
        info.push(("Kundennr.".into(), contact.number.clone()));
    }
    let data = json!({
        "layout": layout_v,
        "company": company_json(&s, &layout),
        "doc": {
            "kind": "mandate",
            "label": title,
            "title": title,
            "heading": title,
            "number": m.reference,
            "is_draft": false,
            "recipient": contact.address_block(None, &c.country).lines().collect::<Vec<_>>(),
            "info": info,
            "intro": text,
            "closing": "\n\n\n_______________________________________\nOrt, Datum\n\n\n\n_______________________________________\nUnterschrift(en) des Kontoinhabers",
            "columns": { "pos": false, "number": false, "unit": false, "discount": false, "tax": false, "prices": false },
            "price_label": "",
            "total_label": "",
            "rows": [],
            "totals": [],
            "notes": ["Bitte unterschrieben zurücksenden. Das Mandat gilt, bis es widerrufen wird; es erlischt, wenn 36 Monate lang keine Lastschrift eingezogen wird."],
            "girocode": Value::Null,
            "custom_table": {
                "columns": ["", ""],
                "align": ["left", "left"],
                "rows": rows,
            },
        }
    });
    let mut job = RenderJob::new(super::DOCUMENT_TEMPLATE, data);
    job.files.extend(files);
    Ok(job)
}

/// Sample document for the layout designer preview.
pub fn sample_document() -> (Document, Vec<DocumentLine>) {
    use rust_decimal_macros::dec;
    let today = util::today();
    let doc = Document {
        doc_type: "invoice".into(),
        kind: "standard".into(),
        status: "open".into(),
        number: "RE-2026-0042".into(),
        address: "Musterfirma GmbH\nFrau Erika Mustermann\nBeispielstraße 12\n10115 Berlin".into(),
        contact_person: "Erika Mustermann".into(),
        date: today,
        service_date: Some(today),
        due_date: Some(today + chrono::Duration::days(14)),
        payment_terms_days: Some(14),
        title: "Rechnung".into(),
        reference: "Bestellung 4711".into(),
        intro_text: "Sehr geehrte Frau Mustermann,\n\nvielen Dank für Ihren Auftrag. Wir erlauben uns, Ihnen folgende Leistungen in Rechnung zu stellen:".into(),
        closing_text: "Bitte überweisen Sie den Rechnungsbetrag innerhalb von 14 Tagen auf das unten angegebene Konto.\n\nMit freundlichen Grüßen".into(),
        tax_mode: "standard".into(),
        currency: "EUR".into(),
        ..Default::default()
    };
    let item = |name: &str, desc: &str, qty: Decimal, unit: &str, price: Decimal, rate: Decimal, nr: &str| DocumentLine {
        line_type: "item".into(),
        article_number: nr.into(),
        name: name.into(),
        description: desc.into(),
        quantity: qty,
        unit: unit.into(),
        unit_price: price,
        tax_rate: rate,
        ..Default::default()
    };
    let lines = vec![
        item("Konzeption und Workshop", "Anforderungsanalyse inkl. Dokumentation", dec!(8), "Std", dec!(95), dec!(19), "DL-100"),
        item("Webdesign Startseite", "Responsive Layout, 2 Korrekturschleifen", dec!(1), "Pauschal", dec!(1250), dec!(19), "DL-200"),
        item("Fachbuch „Gutes Design“", "", dec!(2), "Stk", dec!(39.90), dec!(7), "ART-0007"),
    ];
    let mut doc = doc;
    let mut l = lines.clone();
    let t = calc::calculate(&doc, &mut l);
    calc::apply_totals(&mut doc, &t);
    (doc, l)
}

// ---------------------------------------------------------------------------
// Reports (generic tables)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ReportColumn {
    pub label: String,
    pub align: String,
    pub width: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportTableRow {
    pub cells: Vec<String>,
    pub style: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportSection {
    pub title: String,
    pub columns: Vec<ReportColumn>,
    pub rows: Vec<ReportTableRow>,
}

pub fn col(label: &str, align: &str, width: &str) -> ReportColumn {
    ReportColumn { label: label.into(), align: align.into(), width: width.into() }
}

pub fn trow(cells: Vec<String>, style: &str) -> ReportTableRow {
    ReportTableRow { cells, style: style.into() }
}

pub fn report_job(
    conn: &Connection,
    title: &str,
    subtitle: &str,
    landscape: bool,
    sections: Vec<ReportSection>,
    summary: Vec<(String, String)>,
    notes: Vec<String>,
) -> Result<RenderJob> {
    let s = cfg::load(conn)?;
    let mut tax = Vec::new();
    if !s.company.tax_number.is_empty() {
        tax.push(format!("Steuernummer {}", s.company.tax_number));
    }
    if !s.company.vat_id.is_empty() {
        tax.push(format!("USt-IdNr. {}", s.company.vat_id));
    }
    let tax_line = tax.join(" · ");
    let data = json!({
        "title": title,
        "subtitle": subtitle,
        "company": s.company.name,
        "company_line": format!("{} · {} · {} {}", s.company.name, s.company.street, s.company.zip, s.company.city),
        "tax_line": tax_line,
        "generated": format!("Erstellt am {}", chrono::Local::now().format("%d.%m.%Y %H:%M")),
        "landscape": landscape,
        "sections": sections,
        "summary": summary,
        "notes": notes,
    });
    Ok(RenderJob::new(super::REPORT_TEMPLATE, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(doc_type: &str, status: &str, signature: bool) -> RenderJob {
        let conn = crate::db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        cfg::seed_test_company(&conn);
        let (mut doc, lines) = sample_document();
        doc.doc_type = doc_type.into();
        doc.status = status.into();
        let layout = LayoutConfig { show_signature: signature, ..Default::default() };
        document_job(&conn, Path::new("."), &doc, &lines, Some(layout)).unwrap()
    }

    fn signature(job: &RenderJob) -> bool {
        let data: Value = serde_json::from_slice(&job.files["data.json"]).unwrap();
        data["doc"]["signature"].as_bool().unwrap()
    }

    #[test]
    fn signature_field_is_off_unless_the_layout_enables_it() {
        // layouts stored before the option existed
        let mut stored = serde_json::to_value(LayoutConfig::default()).unwrap();
        stored.as_object_mut().unwrap().remove("show_signature");
        assert!(!serde_json::from_value::<LayoutConfig>(stored).unwrap().show_signature);
        assert!(!signature(&job("quote", "open", false)));
    }

    #[test]
    fn only_open_and_draft_quotes_get_a_signature_field() {
        assert!(signature(&job("quote", "open", true)));
        assert!(signature(&job("quote", "draft", true)));
        assert!(!signature(&job("quote", "accepted", true)));
        assert!(!signature(&job("quote", "rejected", true)));
        assert!(!signature(&job("invoice", "open", true)));
    }

    #[test]
    fn signature_field_changes_the_rendered_quote() {
        let engine = crate::pdf::PdfEngine::new();
        let pages = |signature| {
            let j = job("quote", "open", signature);
            engine.svgs(&engine.compile(j.source, j.files).unwrap())
        };
        assert_ne!(pages(true), pages(false));
    }
}
