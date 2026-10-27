// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Demo data: a realistic business year of a small design & development studio.

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::db::{self, v};
use crate::models::*;
use crate::services::documents::{self, DocumentInput, PartialRequest};
use crate::services::finance::{self, CashEntryInput, PaymentRequest, TransactionInput};
use crate::services::masterdata;
use crate::services::receipts::{self, ReceiptInput};
use crate::services::settings as cfg;
use crate::util::{today, ymd};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % ((hi - lo + 1) as u64)) as i64
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next() as usize % items.len()]
    }
}

fn org(company: &str, street: &str, zip: &str, city: &str, email: &str, customer: bool) -> Contact {
    Contact {
        kind: "organization".into(),
        company: company.into(),
        street: street.into(),
        zip: zip.into(),
        city: city.into(),
        country: "DE".into(),
        email: email.into(),
        is_customer: customer,
        is_supplier: !customer,
        einvoice_format: "pdf".into(),
        ..Default::default()
    }
}

fn person(sal: &str, first: &str, last: &str, pos: &str) -> ContactPerson {
    ContactPerson { salutation: sal.into(), first_name: first.into(), last_name: last.into(), position: pos.into(), primary: true, ..Default::default() }
}

fn article(kind: &str, name: &str, desc: &str, unit: &str, price: Decimal, rate: i64, cat: &str) -> Article {
    Article {
        kind: kind.into(),
        name: name.into(),
        description: desc.into(),
        unit: unit.into(),
        price,
        tax_rate: Decimal::from(rate),
        category: cat.into(),
        ..Default::default()
    }
}

fn clamp_past(d: NaiveDate) -> NaiveDate {
    d.min(today())
}

pub fn load(conn: &Connection) -> Result<()> {
    let mut s = cfg::load(conn)?;
    let chart = s.tax.chart.clone();
    let acc = |r: &str| crate::chart::role(&chart, r);
    let num = |skr03: &str, skr04: &str| if chart == "SKR04" { skr04.to_string() } else { skr03.to_string() };
    if s.company.name.is_empty() {
        s.company = CompanySettings {
            // A sole proprietorship – the demo uses the EÜR, which a GmbH may not.
            name: "Lichtwerk Design & Entwicklung".into(),
            legal_form: "Einzelunternehmen".into(),
            owner: "Jana Lichtenberg".into(),
            street: "Hafenstraße 18".into(),
            zip: "20457".into(),
            city: "Hamburg".into(),
            country: "DE".into(),
            phone: "+49 40 1234567-0".into(),
            email: "rechnung@lichtwerk-design.de".into(),
            website: "www.lichtwerk-design.de".into(),
            tax_number: "27/123/45678".into(),
            vat_id: "DE298765432".into(),
            register_court: String::new(),
            register_number: String::new(),
            bank_name: "Hamburger Sparkasse".into(),
            iban: "DE89370400440532013000".into(),
            bic: "COBADEFFXXX".into(),
            contact_name: "Jana Lichtenberg".into(),
            logo_file_id: None,
        };
        cfg::save(conn, &s)?;
    }
    let small_business = s.tax.small_business;
    let year = today().year();
    let mut rng = Rng(20260514);

    // ---------------------------------------------------------------- Kontakte
    let mut customers: Vec<Contact> = Vec::new();
    let mut c1 = org("Nordlicht Reisen GmbH", "Jungfernstieg 7", "20354", "Hamburg", "buchhaltung@nordlicht-reisen.de", true);
    c1.persons = vec![person("Frau", "Sabine", "Kröger", "Marketingleitung")];
    c1.phone = "+49 40 555 123".into();
    let mut c2 = org("Bäckerei Hansen KG", "Mühlenweg 3", "24103", "Kiel", "info@baeckerei-hansen.de", true);
    c2.persons = vec![person("Herr", "Ole", "Hansen", "Inhaber")];
    let mut c3 = org("Stadt Lüneburg – Fachbereich Kultur", "Am Ochsenmarkt 1", "21335", "Lüneburg", "e-rechnung@stadt.lueneburg.de", true);
    c3.leitweg_id = "03355000-12345-67".into();
    c3.einvoice_format = "xrechnung".into();
    c3.persons = vec![ContactPerson { title: "Dr.".into(), ..person("Frau", "Miriam", "Engel", "Fachbereichsleitung") }];
    c3.payment_terms_days = Some(30);
    let mut c4 = org("Alpenblick Software GmbH", "Mariahilfer Straße 45", "1060", "Wien", "office@alpenblick.at", true);
    c4.country = "AT".into();
    c4.vat_id = "ATU12345678".into();
    c4.persons = vec![person("Herr", "Lukas", "Steiner", "CTO")];
    let mut c5 = org("Grünkraft Energie AG", "Am Sandtorkai 50", "20457", "Hamburg", "rechnungseingang@gruenkraft.de", true);
    c5.einvoice_format = "zugferd".into();
    c5.vat_id = "DE312345678".into();
    c5.persons = vec![person("Herr", "Daniel", "Weber", "Einkauf")];
    let mut c6 = org("Weinhandlung Seidel", "Rathausplatz 2", "23552", "Lübeck", "seidel@weinhandlung-seidel.de", true);
    c6.persons = vec![person("Frau", "Petra", "Seidel", "")];
    let mut c7 = Contact { kind: "person".into(), salutation: "Herr".into(), first_name: "Thomas".into(), last_name: "Brandt".into(), street: "Lindenallee 22".into(), zip: "22303".into(), city: "Hamburg".into(), email: "t.brandt@mail.de".into(), is_customer: true, country: "DE".into(), ..Default::default() };
    c7.einvoice_format = "pdf".into();
    let mut c8 = org("Helvetia Consulting AG", "Bahnhofstrasse 10", "8001", "Zürich", "accounting@helvetia-consulting.ch", true);
    c8.country = "CH".into();
    let mut c9 = org("Kaffeerösterei Elbbohne", "Elbchaussee 120", "22763", "Hamburg", "hallo@elbbohne.de", true);
    c9.persons = vec![person("Frau", "Lena", "Martens", "Geschäftsführerin")];
    let mut c10 = org("Praxis Dr. Albers", "Bergstraße 5", "21073", "Hamburg", "praxis@dr-albers.de", true);
    c10.persons = vec![ContactPerson { title: "Dr.".into(), ..person("Herr", "Jan", "Albers", "") }];
    for c in [&mut c1, &mut c2, &mut c3, &mut c4, &mut c5, &mut c6, &mut c7, &mut c8, &mut c9, &mut c10] {
        if c.einvoice_format.is_empty() {
            c.einvoice_format = "pdf".into();
        }
        customers.push(masterdata::save_contact(conn, c.clone())?);
    }
    let mut suppliers: Vec<Contact> = Vec::new();
    let sup = [
        org("Telekom Deutschland GmbH", "Landgrabenweg 151", "53227", "Bonn", "rechnung@telekom.de", false),
        org("Adobe Systems Software Ireland Ltd", "4-6 Riverwalk", "D24", "Dublin", "invoices@adobe.com", false),
        org("Hetzner Online GmbH", "Industriestr. 25", "91710", "Gunzenhausen", "billing@hetzner.com", false),
        org("Büromarkt Nord GmbH", "Wandsbeker Chaussee 80", "22089", "Hamburg", "service@bueromarkt-nord.de", false),
        org("Immobilien Petersen", "Große Elbstraße 14", "22767", "Hamburg", "verwaltung@petersen-immo.de", false),
        org("Steuerberatung Klein & Partner", "Neuer Wall 30", "20354", "Hamburg", "kanzlei@klein-partner.de", false),
        org("Deutsche Bahn AG", "Europaplatz 1", "10557", "Berlin", "", false),
        org("Apple Distribution International", "Hollyhill Industrial Estate", "T23", "Cork", "", false),
    ];
    // Example IBANs (valid check digits) so that SEPA transfers work in the demo.
    let ibans = [
        ("Telekom", "DE55200400000400880000"),
        ("Hetzner", "DE61760501010012345678"),
        ("Büromarkt", "DE34200505501234567890"),
        ("Petersen", "DE07200505500009876543"),
        ("Klein", "DE50380700590123456700"),
    ];
    for mut c in sup {
        if let Some((_, iban)) = ibans.iter().find(|(n, _)| c.company.contains(n)) {
            c.iban = (*iban).into();
        }
        if c.company.contains("Ireland") || c.company.contains("Apple") {
            c.country = "IE".into();
            c.vat_id = if c.company.contains("Adobe") { "IE6364992H".into() } else { "IE9700053D".into() };
        }
        suppliers.push(masterdata::save_contact(conn, c)?);
    }

    // ---------------------------------------------------------------- Artikel
    let arts = vec![
        article("service", "Webentwicklung", "Entwicklung nach Aufwand", "Std", dec!(95), 19, "Entwicklung"),
        article("service", "UX/UI-Design", "Konzeption und Gestaltung von Benutzeroberflächen", "Std", dec!(105), 19, "Design"),
        article("service", "Strategieberatung", "Workshop und Beratung", "Std", dec!(140), 19, "Beratung"),
        article("service", "Logo- und Markenentwicklung", "inkl. 3 Entwürfe und Styleguide", "Pauschal", dec!(2400), 19, "Design"),
        article("service", "Website-Wartung", "Updates, Backups, Monitoring", "Monat", dec!(149), 19, "Wartung"),
        article("service", "Hosting Business", "Managed Hosting inkl. SSL", "Monat", dec!(39), 19, "Hosting"),
        article("service", "Fotografie Tagessatz", "Produkt- und Teamfotografie", "Tag", dec!(890), 19, "Fotografie"),
        article("service", "Schulung", "Redaktionsschulung CMS (halbtags)", "Pauschal", dec!(650), 19, "Schulung"),
        article("product", "Visitenkarten Premium", "500 Stück, 400 g/m², beidseitig", "Stk", dec!(89), 19, "Druck"),
        article("product", "Fachbuch „Designsysteme“", "Hardcover, 320 Seiten", "Stk", dec!(39.90), 7, "Bücher"),
    ];
    let mut articles = Vec::new();
    for a in arts {
        articles.push(masterdata::save_article(conn, a)?);
    }

    let bank_id: i64 = conn.query_row("SELECT id FROM payment_accounts WHERE kind != 'cash' ORDER BY id LIMIT 1", [], |r| r.get(0))?;
    conn.execute(
        "UPDATE payment_accounts SET iban = ?1, bic = ?2, bank_name = ?3 WHERE id = ?4 AND iban = ''",
        rusqlite::params![s.company.iban, s.company.bic, s.company.bank_name, bank_id],
    )?;
    let cash_id: Option<i64> = conn.query_row("SELECT id FROM payment_accounts WHERE kind = 'cash' ORDER BY id LIMIT 1", [], |r| r.get(0)).ok();

    let line_from = |a: &Article, qty: Decimal| DocumentLine {
        line_type: "item".into(),
        article_id: Some(a.id),
        article_number: a.number.clone(),
        name: a.name.clone(),
        description: a.description.clone(),
        quantity: qty,
        unit: a.unit.clone(),
        unit_price: a.price,
        tax_rate: a.tax_rate,
        ..Default::default()
    };

    // Creates, dates and finalizes a document.
    let make_doc = |conn: &Connection, doc_type: &str, contact: &Contact, date: NaiveDate, lines: Vec<DocumentLine>, finalize: bool| -> Result<Document> {
        let mut input = documents::new_template(conn, doc_type, Some(contact.id))?;
        input.doc.date = date;
        if let Some(days) = input.doc.payment_terms_days {
            input.doc.due_date = Some(date + Duration::days(days));
        }
        if doc_type == "quote" {
            input.doc.valid_until = Some(date + Duration::days(30));
        }
        input.doc.service_date = Some(date);
        input.lines = lines;
        let id = documents::create(conn, input)?;
        if finalize {
            documents::finalize_demo(conn, id)?;
        }
        db::get(conn, id)
    };

    let pay = |conn: &Connection, doc: &Document, date: NaiveDate, amount: Option<Decimal>| -> Result<()> {
        finance::pay_document(
            conn,
            doc.id,
            PaymentRequest { account_id: Some(bank_id), date: Some(clamp_past(date)), amount: amount.unwrap_or(doc.gross_total), ..Default::default() },
        )
    };

    // ---------------------------------------------------------------- Rechnungen
    let last_month = today().month();
    let mut invoice_count = 0;
    for month in 1..=last_month {
        let per_month = if month == last_month { 2 } else { rng.range(2, 4) };
        for _ in 0..per_month {
            let day = rng.range(1, 26) as u32;
            let date = ymd(year, month, day);
            if date > today() {
                continue;
            }
            let ci = rng.range(0, customers.len() as i64 - 1) as usize;
            let customer = customers[ci].clone();
            let mut lines = Vec::new();
            let n = rng.range(1, 3);
            for _ in 0..n {
                let a = rng.pick(&articles[..8]).clone();
                let qty = if a.unit == "Std" { Decimal::from(rng.range(4, 32)) } else { Decimal::from(rng.range(1, 3)) };
                if !lines.iter().any(|l: &DocumentLine| l.article_id == Some(a.id)) {
                    lines.push(line_from(&a, qty));
                }
            }
            if rng.range(0, 6) == 0 {
                lines.push(line_from(&articles[9], Decimal::from(rng.range(1, 4))));
            }
            let doc = make_doc(conn, "invoice", &customer, date, lines, true)?;
            invoice_count += 1;
            let days_since = (today() - date).num_days();
            let roll = rng.range(0, 9);
            if days_since > 40 && roll < 9 || days_since > 20 && roll < 6 || days_since > 10 && roll < 3 {
                pay(conn, &doc, date + Duration::days(rng.range(6, 24)), None)?;
            }
        }
    }
    let _ = invoice_count;

    // Partially paid invoice with Skonto
    let d = make_doc(conn, "invoice", &customers[5], ymd(year, 3.min(last_month), 12), vec![line_from(&articles[3], dec!(1))], true)?;
    finance::pay_document(
        conn,
        d.id,
        PaymentRequest { account_id: Some(bank_id), date: Some(clamp_past(ymd(year, 3.min(last_month), 20))), amount: (d.gross_total * dec!(0.98)).round_dp(2), writeoff: "skonto".into(), ..Default::default() },
    )?;
    // Cancelled invoice
    let d = make_doc(conn, "invoice", &customers[1], ymd(year, 2.min(last_month), 9), vec![line_from(&articles[6], dec!(1))], true)?;
    documents::cancel(conn, d.id)?;
    // Credit note
    if let Some(inv) = db::query_one::<Document>(conn, "WHERE doc_type = 'invoice' AND status = 'paid' AND kind = 'standard' ORDER BY date LIMIT 1", &[])? {
        let cn_id = documents::credit_note_from(conn, inv.id)?;
        let mut full = documents::get_full(conn, cn_id)?;
        full.lines.truncate(1);
        if let Some(l) = full.lines.first_mut() {
            l.quantity = Decimal::ONE;
            l.name = format!("Gutschrift: {}", l.name);
        }
        full.doc.date = clamp_past(inv.date + Duration::days(20));
        full.doc.due_date = Some(full.doc.date + Duration::days(full.doc.payment_terms_days.unwrap_or(14)));
        full.doc.reference = inv.number.clone();
        documents::update(conn, cn_id, DocumentInput { doc: full.doc, lines: full.lines })?;
        documents::finalize_demo(conn, cn_id)?;
    }
    // Overdue invoices for dunning
    for (ci, m_back, art) in [(0usize, 2i64, 2usize), (8, 1, 0)] {
        let date = today() - Duration::days(30 * m_back + 12);
        make_doc(conn, "invoice", &customers[ci], date, vec![line_from(&articles[art], dec!(12))], true)?;
    }
    // Draft invoice
    make_doc(conn, "invoice", &customers[6], today(), vec![line_from(&articles[1], dec!(6)), line_from(&articles[8], dec!(1))], false)?;

    // ---------------------------------------------------------------- Angebote & Aufträge
    let q1 = make_doc(conn, "quote", &customers[4], today() - Duration::days(9), vec![line_from(&articles[2], dec!(8)), line_from(&articles[1], dec!(24))], true)?;
    let _ = q1;
    let q2 = make_doc(conn, "quote", &customers[8], today() - Duration::days(45), vec![line_from(&articles[6], dec!(2))], true)?;
    documents::set_status(conn, q2.id, "rejected")?;
    make_doc(conn, "quote", &customers[9], today() - Duration::days(3), vec![line_from(&articles[0], dec!(40)), line_from(&articles[4], dec!(12))], true)?;
    make_doc(conn, "quote", &customers[2], today(), vec![line_from(&articles[7], dec!(2))], false)?;

    // Project with partial invoices and final invoice
    let project_start = ymd(year, 4.min(last_month), 8);
    let pq = make_doc(conn, "quote", &customers[0], project_start, vec![line_from(&articles[3], dec!(1)), line_from(&articles[0], dec!(80)), line_from(&articles[1], dec!(40))], true)?;
    let order_id = documents::convert(conn, pq.id, "order")?;
    let mut order = documents::get_full(conn, order_id)?;
    order.doc.date = project_start + Duration::days(5);
    documents::update(conn, order_id, DocumentInput { doc: order.doc, lines: order.lines })?;
    let order = documents::finalize_demo(conn, order_id)?;
    let mut partial_dates = vec![project_start + Duration::days(10), project_start + Duration::days(60)];
    partial_dates.retain(|d| *d <= today());
    for (i, pd) in partial_dates.iter().enumerate() {
        let pid = documents::create_partial(conn, order.id, PartialRequest { mode: "percent".into(), value: if i == 0 { dec!(30) } else { dec!(40) } })?;
        let mut p = documents::get_full(conn, pid)?;
        p.doc.date = *pd;
        p.doc.due_date = Some(*pd + Duration::days(14));
        p.doc.service_date = Some(*pd);
        documents::update(conn, pid, DocumentInput { doc: p.doc, lines: p.lines })?;
        let p = documents::finalize_demo(conn, pid)?;
        pay(conn, &p, *pd + Duration::days(9), None)?;
    }
    if project_start + Duration::days(120) <= today() {
        let fid = documents::create_final(conn, order.id)?;
        let mut f = documents::get_full(conn, fid)?;
        let fd = project_start + Duration::days(120);
        f.doc.date = fd;
        f.doc.due_date = Some(fd + Duration::days(14));
        f.doc.service_date = Some(fd);
        documents::update(conn, fid, DocumentInput { doc: f.doc, lines: f.lines })?;
        documents::finalize_demo(conn, fid)?;
    }
    // Order confirmation + delivery note
    let ab = make_doc(conn, "order", &customers[9], today() - Duration::days(6), vec![line_from(&articles[8], dec!(2)), line_from(&articles[9], dec!(5))], true)?;
    let ls_id = documents::convert(conn, ab.id, "delivery")?;
    documents::finalize_demo(conn, ls_id)?;

    // SEPA-Lastschrift: a maintenance contract collected monthly as Abo-Rechnung.
    let mut s = cfg::load(conn)?;
    if s.debit.creditor_id.is_empty() {
        // Example Gläubiger-ID of the Deutsche Bundesbank.
        s.debit.creditor_id = "DE98ZZZ09999999999".into();
        cfg::save(conn, &s)?;
    }
    let albers = &customers[9];
    crate::services::debit::save_mandate(
        conn,
        Mandate {
            contact_id: albers.id,
            reference: crate::services::debit::suggest_reference(conn, albers.id)?,
            signed_on: today() - Duration::days(40),
            scheme: "CORE".into(),
            recurring: true,
            iban: "DE02120300000000202051".into(),
            ..Default::default()
        },
    )?;
    let care = DocumentLine { line_type: "item".into(), name: "Website-Wartung und Hosting {{Monat}}".into(), quantity: dec!(1), unit: "Monat".into(), unit_price: dec!(89), tax_rate: dec!(19), ..Default::default() };
    let contract = make_doc(conn, "invoice", albers, today() - Duration::days(2), vec![care], true)?;
    crate::services::recurring::save(
        conn,
        RecurringInvoice { template_id: contract.id, cycle: "monthly".into(), next_date: crate::util::add_months(contract.date, 1), auto_finalize: true, active: true, ..Default::default() },
    )?;

    // ---------------------------------------------------------------- Belege
    let rl = |account: String, desc: &str, gross: Decimal, rate: i64| ReceiptLine {
        account,
        description: desc.into(),
        gross,
        net: gross,
        tax_rate: Decimal::from(rate),
        ..Default::default()
    };
    let mk_receipt = |conn: &Connection, supplier: &Contact, date: NaiveDate, nr: &str, mode: &str, lines: Vec<ReceiptLine>, paid: Option<(i64, i64)>| -> Result<i64> {
        let gross_mode = mode == "standard";
        let r = Receipt {
            receipt_type: "expense".into(),
            contact_id: Some(supplier.id),
            supplier_name: supplier.display_name(),
            invoice_number: nr.into(),
            date,
            due_date: Some(date + Duration::days(14)),
            description: lines.first().map(|l| l.description.clone()).unwrap_or_default(),
            tax_mode: mode.into(),
            prices_gross: gross_mode,
            ..Default::default()
        };
        let payment = paid.map(|(acc, days)| PaymentRequest { account_id: Some(acc), date: Some(clamp_past(date + Duration::days(days))), ..Default::default() });
        receipts::create(conn, ReceiptInput { receipt: r, lines, payment })
    };
    for month in 1..=last_month {
        let first = ymd(year, month, 1);
        if first > today() {
            break;
        }
        let paid = |d: i64| if ymd(year, month, 1) + Duration::days(d) <= today() { Some((bank_id, d)) } else { None };
        mk_receipt(conn, &suppliers[4], first, &format!("M-{year}-{month:02}"), "standard", vec![rl(num("4210", "6310"), "Büromiete", dec!(1450), 0)], paid(2))?;
        mk_receipt(conn, &suppliers[0], ymd(year, month, 5.min(28)), &format!("T{}{:02}88", year, month), "standard", vec![rl(num("4925", "6810"), "Internet & Telefon Business", dec!(59.95), 19)], paid(10))?;
        let adobe = ReceiptLine { account: num("4964", "6837"), description: "Creative Cloud Teams (3 Lizenzen)".into(), net: dec!(214.17), tax_rate: dec!(19), ..Default::default() };
        mk_receipt(conn, &suppliers[1], ymd(year, month, 12.min(28)), &format!("IEE{year}{month:02}4471"), "rc_eu", vec![adobe], paid(1))?;
        mk_receipt(conn, &suppliers[2], ymd(year, month, 3), &format!("R{}{:02}0193", year, month), "standard", vec![rl(num("4925", "6810"), "Dedicated Server & Cloud", dec!(83.30), 19)], paid(5))?;
        if month % 3 == 0 {
            mk_receipt(conn, &suppliers[5], ymd(year, month, 20), &format!("KP-{year}-{month}"), "standard", vec![rl(num("4955", "6830"), "Finanzbuchhaltung Quartal", dec!(535.50), 19)], paid(12))?;
        }
        if rng.range(0, 1) == 0 {
            mk_receipt(conn, &suppliers[3], ymd(year, month, rng.range(6, 25) as u32), &format!("BN{}", rng.range(10000, 99999)), "standard", vec![rl(num("4930", "6815"), "Büromaterial und Papier", Decimal::from(rng.range(25, 140)) + dec!(0.90), 19)], paid(0))?;
        }
        if month % 2 == 0 {
            let travel = vec![rl(num("4673", "6673"), "Bahnfahrt Hamburg–Berlin (Kundentermin)", dec!(129.90), 19), rl(num("4676", "6680"), "Hotelübernachtung Berlin", dec!(118.00), 7)];
            mk_receipt(conn, &suppliers[6], ymd(year, month, rng.range(5, 25) as u32), &format!("DB{}", rng.range(100000, 999999)), "standard", travel, paid(0))?;
        }
    }
    // Annual insurance, professional literature, assets
    mk_receipt(conn, &suppliers[3], ymd(year, 1, 15), "VS-2026-001", "standard", vec![rl(num("4360", "6400"), "Betriebshaftpflicht Jahresbeitrag", dec!(486.00), 0)], Some((bank_id, 3)))?;
    let laptop = ReceiptLine { account: num("0420", "0650"), description: "MacBook Pro 16\" M5".into(), gross: dec!(3199.00), net: dec!(3199.00), tax_rate: dec!(19), asset_life_years: 3, ..Default::default() };
    let monitor = ReceiptLine { account: num("0480", "0670"), description: "4K-Monitor 27\"".into(), gross: dec!(415.31), net: dec!(415.31), tax_rate: dec!(19), ..Default::default() };
    mk_receipt(conn, &suppliers[7], ymd(year, 2.min(last_month), 18), "AP-77821", "standard", vec![laptop, monitor], Some((bank_id, 1)))?;
    if last_month >= 7 {
        mk_receipt(conn, &suppliers[3], ymd(year, 7, 2), "BN-88123", "standard", vec![rl(num("0480", "0670"), "Ergonomischer Bürostuhl", dec!(821.10), 19)], Some((bank_id, 4)))?;
    }
    // Open supplier invoices
    mk_receipt(conn, &suppliers[5], today() - Duration::days(5), "KP-JA-2026", "standard", vec![rl(num("4957", "6827"), "Vorbereitung Jahresabschluss", dec!(892.50), 19)], None)?;
    mk_receipt(conn, &suppliers[3], today() - Duration::days(20), "BN-91002", "standard", vec![rl(num("4930", "6815"), "Druckerpatronen", dec!(189.90), 19)], None)?;

    // ---------------------------------------------------------------- Bank: Gebühren, offene Umsätze
    for month in [1u32, 4, 7, 10] {
        let d = ymd(year, month, 28);
        if d > today() {
            continue;
        }
        finance::create(
            conn,
            TransactionInput {
                tx: Transaction { account_id: bank_id, date: d, amount: dec!(-14.90), counterparty: "Hamburger Sparkasse".into(), purpose: "Kontoführungsentgelt".into(), source: "manual".into(), ..Default::default() },
                allocations: vec![Allocation { kind: "account".into(), account: acc("geldverkehr"), amount: dec!(-14.90), description: "Kontoführung".into(), ..Default::default() }],
            },
        )?;
    }
    // Unassigned bank movements (simulated import)
    if let Some(open) = db::query_one::<Document>(conn, "WHERE doc_type = 'invoice' AND status = 'open' AND kind = 'standard' ORDER BY date LIMIT 1", &[])? {
        let name = open.address.lines().next().unwrap_or_default().to_string();
        finance::create(
            conn,
            TransactionInput {
                tx: Transaction { account_id: bank_id, date: today() - Duration::days(1), amount: open.gross_total, counterparty: name, purpose: format!("Zahlung Rechnung {}", open.number), source: "import".into(), import_hash: "demo1".into(), ..Default::default() },
                allocations: vec![],
            },
        )?;
    }
    finance::create(
        conn,
        TransactionInput {
            tx: Transaction { account_id: bank_id, date: today() - Duration::days(2), amount: dec!(-39.00), counterparty: "PayPal Europe".into(), purpose: "Stock-Foto Lizenz 1029384".into(), source: "import".into(), import_hash: "demo2".into(), ..Default::default() },
            allocations: vec![],
        },
    )?;

    // ---------------------------------------------------------------- Kasse
    if let Some(cash) = cash_id {
        let start = ymd(year, 1, 2);
        finance::create(
            conn,
            TransactionInput {
                tx: Transaction { account_id: cash, date: start, amount: dec!(300), purpose: "Bargeldabhebung für Kasse".into(), source: "cash".into(), ..Default::default() },
                allocations: vec![Allocation { kind: "account".into(), account: acc("transit"), amount: dec!(300), description: "Geldtransit Bank → Kasse".into(), ..Default::default() }],
            },
        )?;
        finance::create(
            conn,
            TransactionInput {
                tx: Transaction { account_id: bank_id, date: start, amount: dec!(-300), counterparty: "Bargeldauszahlung".into(), purpose: "Abhebung Geldautomat".into(), source: "manual".into(), ..Default::default() },
                allocations: vec![Allocation { kind: "account".into(), account: acc("transit"), amount: dec!(-300), description: "Geldtransit Bank → Kasse".into(), ..Default::default() }],
            },
        )?;
        let entries = [
            (12, "out", dec!(17.40), "Porto Einschreiben", num("4910", "6800"), 0),
            (25, "out", dec!(9.80), "Parkgebühren Kundentermin", num("4530", "6530"), 19),
            (41, "out", dec!(64.50), "Geschäftsessen mit Kunde (Bewirtung)", num("4650", "6640"), 19),
            (77, "in", dec!(79.80), "Barverkauf 2× Fachbuch", acc("erl7"), 7),
            (102, "out", dec!(23.99), "Büromaterial Schreibwaren", num("4930", "6815"), 19),
            (150, "out", dec!(12.00), "Taxi zum Bahnhof", num("4673", "6673"), 7),
            (190, "out", dec!(34.90), "Blumen Empfang (Raumkosten)", num("4200", "6305"), 7),
        ];
        for (days, dir, amount, text, account, rate) in entries {
            let d = start + Duration::days(days);
            if d > today() {
                continue;
            }
            finance::create_cash_entry(
                conn,
                CashEntryInput { account_id: cash, date: Some(d), direction: dir.into(), amount, description: text.into(), counter_account: account, tax_rate: Decimal::from(rate), ..Default::default() },
            )?;
        }
    }

    // ---------------------------------------------------------------- Steuern: Vorauszahlungen und Meldungen
    // Last, once every booking of the quarters exists (the cash book carries VAT as well): the filed
    // amounts then match the returns, and the demo does not start with "changed after filing".
    let quarters_done = (last_month - 1) / 3;
    for q in 1..=quarters_done {
        let period = format!("Q{q}");
        let rep = crate::services::reports::ustva(conn, year, &period)?;
        if small_business || rep.zahllast.is_zero() {
            continue;
        }
        let due = rep.due_date;
        if due > today() {
            continue;
        }
        finance::create(
            conn,
            TransactionInput {
                tx: Transaction { account_id: bank_id, date: due, amount: -rep.zahllast, counterparty: "Finanzamt Hamburg-Mitte".into(), purpose: format!("USt-VA {period}/{year} StNr 27/123/45678"), source: "manual".into(), ..Default::default() },
                allocations: vec![Allocation { kind: "account".into(), account: acc("ust_vz"), amount: -rep.zahllast, description: format!("Umsatzsteuer-Vorauszahlung {period}/{year}"), ..Default::default() }],
            },
        )?;
        let mut f = UstvaFiling { id: 0, year: year as i64, period, submitted_at: (due - Duration::days(3)).to_string(), amount: rep.zahllast, note: "Über ELSTER übermittelt".into() };
        db::insert(conn, &mut f)?;
    }
    for q in 1..=quarters_done {
        let period = format!("Q{q}");
        let rep = crate::services::zm::report(conn, year, &period)?;
        if !small_business && !rep.rows.is_empty() && rep.due_date <= today() {
            crate::services::zm::file(conn, year, &period, rep.due_date - Duration::days(5), "Über ELSTER übermittelt".into())?;
        }
    }
    renumber_chronologically(conn)?;
    crate::services::posting::rebuild_all(conn)?;
    let _ = v(0i64);
    Ok(())
}

/// The demo creates some documents out of date order (project invoices, credit notes,
/// additional receipts). Numbers must ascend with the date (GoBD), so the numbers within each
/// series are reassigned chronologically – including every text that mentions them.
fn renumber_chronologically(conn: &Connection) -> Result<()> {
    let series = |n: &str| n.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for table in ["documents", "receipts"] {
        let mut rows: Vec<(i64, String, String)> = Vec::new();
        let mut stmt = conn.prepare(&format!("SELECT id, number, date FROM {table} WHERE number != '' ORDER BY date, id"))?;
        for r in stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))? {
            rows.push(r?);
        }
        let mut groups: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
        for (_, number, _) in &rows {
            groups.entry(series(number)).or_default().push(number.clone());
        }
        for (key, chronological) in groups {
            let mut sorted = chronological.clone();
            sorted.sort();
            for (old, new) in chronological.into_iter().zip(sorted) {
                if old != new {
                    debug_assert!(series(&new) == key);
                    pairs.push((old, new));
                }
            }
        }
    }
    if pairs.is_empty() {
        return Ok(());
    }
    let columns: &[(&str, &[&str])] = &[
        ("documents", &["number", "reference", "intro_text", "closing_text", "notes"]),
        ("document_lines", &["name", "description"]),
        ("receipts", &["number", "description", "notes"]),
        ("transactions", &["purpose"]),
        ("allocations", &["description"]),
        ("dunnings", &["title", "text"]),
        ("activities", &["message"]),
    ];
    // Two phases via unique placeholders, so swapped numbers do not overwrite each other.
    for phase in 0..2 {
        for (i, (old, new)) in pairs.iter().enumerate() {
            let token = format!("{i}");
            let (from, to) = if phase == 0 { (old.as_str(), token.as_str()) } else { (token.as_str(), new.as_str()) };
            for (table, cols) in columns {
                for col in *cols {
                    conn.execute(&format!("UPDATE {table} SET {col} = REPLACE({col}, ?1, ?2) WHERE instr({col}, ?1) > 0"), [from, to])?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_with_incomplete_company() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        let mut s = cfg::load(&conn).unwrap();
        s.company.name = "Nur ein Name".into();
        cfg::save(&conn, &s).unwrap();
        finance::save_account(&conn, PaymentAccount { name: "Bank".into(), kind: "bank".into(), ..Default::default() }).unwrap();
        load(&conn).unwrap();
        assert!(db::scalar_i64(&conn, "SELECT COUNT(*) FROM documents WHERE status != 'draft'", &[]).unwrap() > 0);
    }

    #[test]
    fn filed_returns_match_the_loaded_data() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        // As the onboarding creates them: the cash book entries carry VAT as well.
        finance::save_account(&conn, PaymentAccount { name: "Bank".into(), kind: "bank".into(), ..Default::default() }).unwrap();
        finance::save_account(&conn, PaymentAccount { name: "Kasse".into(), kind: "cash".into(), ..Default::default() }).unwrap();
        load(&conn).unwrap();
        for p in crate::services::reports::ustva_periods(&conn, today().year()).unwrap() {
            if let Some(f) = p.filing {
                assert_eq!(f.amount, p.zahllast, "UStVA {}", p.period);
            }
        }
        let todos: Vec<String> = crate::services::reports::dashboard(&conn).unwrap().todos.into_iter().map(|t| t.kind).collect();
        assert!(!todos.iter().any(|k| k.ends_with("_changed")), "{todos:?}");
    }
}
