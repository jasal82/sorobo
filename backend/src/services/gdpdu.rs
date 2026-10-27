// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Datenträgerüberlassung for tax audits (§ 147 Abs. 6 AO, GoBD): the data of a fiscal year as
//! CSV tables with an `index.xml` according to the "Beschreibungsstandard für die
//! Datenträgerüberlassung" (DTD gdpdu-01-09-2004), packed into one ZIP file.

use std::collections::HashMap;
use std::io::Write;

use anyhow::Result;
use chrono::{Datelike, NaiveDate, Timelike};
use rusqlite::Connection;
use rust_decimal::Decimal;

use crate::db::{self, v};
use crate::models::*;
use crate::services::documents::type_label;
use crate::services::settings as cfg;
use crate::util::{fmt_date, now_str, ymd};

const DTD_NAME: &str = "gdpdu-01-09-2004.dtd";
const DTD: &str = include_str!("gdpdu-01-09-2004.dtd");

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Date,
    /// Number with this many decimal places.
    Num(u32),
}

struct Col {
    name: &'static str,
    description: &'static str,
    kind: Kind,
}

const fn col(name: &'static str, kind: Kind) -> Col {
    Col { name, description: "", kind }
}

const fn col_d(name: &'static str, description: &'static str, kind: Kind) -> Col {
    Col { name, description, kind }
}

use Kind::{Date, Num, Text};

struct Table {
    file: &'static str,
    name: &'static str,
    description: String,
    /// The first `keys` columns form the primary key.
    keys: usize,
    columns: Vec<Col>,
    /// (columns, referenced table)
    foreign_keys: Vec<(&'static [&'static str], &'static str)>,
    rows: Vec<Vec<String>>,
}

fn text(s: &str) -> String {
    // Line breaks (addresses, purposes) would split the record.
    s.split(['\r', '\n']).map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(", ")
}

fn num(d: Decimal, places: u32) -> String {
    format!("{:.*}", places as usize, d.round_dp(places)).replace('.', ",")
}

fn money(d: Decimal) -> String {
    num(d, 2)
}

fn date(d: NaiveDate) -> String {
    fmt_date(d)
}

fn date_opt(d: Option<NaiveDate>) -> String {
    d.map(fmt_date).unwrap_or_default()
}

fn id_opt(id: Option<i64>) -> String {
    id.map(|i| i.to_string()).unwrap_or_default()
}

fn yes_no(b: bool) -> String {
    if b { "ja" } else { "nein" }.into()
}

impl Table {
    fn csv(&self) -> String {
        let mut out = String::new();
        let header: Vec<String> = self.columns.iter().map(|c| format!("\"{}\"", c.name)).collect();
        out.push_str(&header.join(";"));
        out.push_str("\r\n");
        for row in &self.rows {
            let cells: Vec<String> = row
                .iter()
                .zip(&self.columns)
                .map(|(val, c)| match c.kind {
                    Text => format!("\"{}\"", val.replace('"', "\"\"")),
                    _ => val.clone(),
                })
                .collect();
            out.push_str(&cells.join(";"));
            out.push_str("\r\n");
        }
        out
    }

    fn xml(&self, from: NaiveDate, to: NaiveDate) -> String {
        let mut x = String::new();
        x.push_str("    <Table>\n");
        x.push_str(&format!("      <URL>{}</URL>\n      <Name>{}</Name>\n      <Description>{}</Description>\n", self.file, xml_escape(self.name), xml_escape(&self.description)));
        x.push_str(&format!("      <Validity>\n        <Range>\n          <From>{}</From>\n          <To>{}</To>\n        </Range>\n        <Format>DD.MM.YYYY</Format>\n      </Validity>\n", date(from), date(to)));
        x.push_str("      <UTF8/>\n      <DecimalSymbol>,</DecimalSymbol>\n      <DigitGroupingSymbol>.</DigitGroupingSymbol>\n");
        // The first record holds the column names.
        x.push_str("      <Range>\n        <From>2</From>\n      </Range>\n");
        x.push_str("      <VariableLength>\n        <ColumnDelimiter>;</ColumnDelimiter>\n        <RecordDelimiter>&#13;&#10;</RecordDelimiter>\n        <TextEncapsulator>\"</TextEncapsulator>\n");
        for (i, c) in self.columns.iter().enumerate() {
            let tag = if i < self.keys { "VariablePrimaryKey" } else { "VariableColumn" };
            x.push_str(&format!("        <{tag}>\n          <Name>{}</Name>\n", xml_escape(c.name)));
            if !c.description.is_empty() {
                x.push_str(&format!("          <Description>{}</Description>\n", xml_escape(c.description)));
            }
            match c.kind {
                Text => x.push_str("          <AlphaNumeric/>\n"),
                Date => x.push_str("          <Date>\n            <Format>DD.MM.YYYY</Format>\n          </Date>\n"),
                Num(0) => x.push_str("          <Numeric/>\n"),
                Num(p) => x.push_str(&format!("          <Numeric>\n            <Accuracy>{p}</Accuracy>\n          </Numeric>\n")),
            }
            x.push_str(&format!("        </{tag}>\n"));
        }
        for (cols, table) in &self.foreign_keys {
            x.push_str("        <ForeignKey>\n");
            for c in *cols {
                x.push_str(&format!("          <Name>{}</Name>\n", xml_escape(c)));
            }
            x.push_str(&format!("          <References>{}</References>\n        </ForeignKey>\n", xml_escape(table)));
        }
        x.push_str("      </VariableLength>\n    </Table>\n");
        x
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn source_label(t: &str) -> &'static str {
    match t {
        "document" => "Rechnung/Gutschrift",
        "receipt" => "Beleg",
        "transaction" => "Zahlung",
        "writeoff" => "Ausbuchung",
        "asset" => "Abschreibung",
        "manual" => "Manuelle Buchung",
        "opening" => "Eröffnungsbuchung",
        _ => "Sonstige",
    }
}

fn tables(conn: &Connection, from: NaiveDate, to: NaiveDate) -> Result<Vec<Table>> {
    let s = cfg::load(conn)?;
    let range = [v(from), v(to)];
    let locked = |d: NaiveDate| s.tax.locked_until.is_some_and(|l| d <= l);

    let accounts = db::query::<Account>(conn, "ORDER BY number", &[])?;
    let konten = Table {
        file: "konten.csv",
        name: "Konten",
        description: format!("Kontenrahmen {}", s.tax.chart),
        keys: 1,
        columns: vec![
            col("Kontonummer", Text),
            col("Bezeichnung", Text),
            col_d("Kontoart", "asset, current, liability, equity, private, tax, revenue, expense, neutral", Text),
            col("Steuerschluessel", Text),
            col("Steuersatz", Num(2)),
            col("Aktiv", Text),
        ],
        foreign_keys: vec![],
        rows: accounts.iter().map(|a| vec![a.number.clone(), text(&a.name), a.kind.clone(), a.tax_code.clone(), a.tax_rate.map(money).unwrap_or_default(), yes_no(a.active)]).collect(),
    };

    let contacts = db::query::<Contact>(conn, "ORDER BY id", &[])?;
    let kontakte = Table {
        file: "kontakte.csv",
        name: "Kontakte",
        description: "Kunden und Lieferanten".into(),
        keys: 1,
        columns: vec![
            col("KontaktId", Num(0)),
            col("Kontaktnummer", Text),
            col("Name", Text),
            col("Kunde", Text),
            col("Lieferant", Text),
            col("Strasse", Text),
            col("PLZ", Text),
            col("Ort", Text),
            col("Land", Text),
            col("UStIdNr", Text),
            col("Steuernummer", Text),
            col("IBAN", Text),
        ],
        foreign_keys: vec![],
        rows: contacts
            .iter()
            .map(|c| {
                vec![
                    c.id.to_string(),
                    c.number.clone(),
                    text(&c.display_name()),
                    yes_no(c.is_customer),
                    yes_no(c.is_supplier),
                    text(&c.street),
                    c.zip.clone(),
                    text(&c.city),
                    c.country.clone(),
                    c.vat_id.clone(),
                    c.tax_number.clone(),
                    c.iban.clone(),
                ]
            })
            .collect(),
    };

    let entries = db::query::<JournalEntry>(conn, "WHERE date >= ?1 AND date <= ?2 ORDER BY date, id", &range)?;
    let mut lines: HashMap<i64, Vec<JournalLine>> = HashMap::new();
    for l in db::query::<JournalLine>(
        conn,
        "WHERE entry_id IN (SELECT id FROM journal_entries WHERE date >= ?1 AND date <= ?2) ORDER BY entry_id, id",
        &range,
    )? {
        lines.entry(l.entry_id).or_default().push(l);
    }
    let mut journal_rows = Vec::new();
    for e in &entries {
        for (i, l) in lines.get(&e.id).into_iter().flatten().enumerate() {
            journal_rows.push(vec![
                e.id.to_string(),
                (i + 1).to_string(),
                date(e.date),
                text(&e.doc_ref),
                text(&e.description),
                l.account.clone(),
                money(l.debit),
                money(l.credit),
                l.tax_code.clone(),
                money(l.base),
                source_label(&e.source_type).into(),
                e.source_id.to_string(),
                id_opt(e.contact_id),
                e.created_at.clone(),
                yes_no(locked(e.date)),
            ]);
        }
    }
    let buchungen = Table {
        file: "buchungen.csv",
        name: "Buchungen",
        description: "Buchungsjournal: jede Zeile ist eine Soll- oder Habenbuchung eines Buchungssatzes".into(),
        keys: 2,
        columns: vec![
            col("BuchungId", Num(0)),
            col("Zeile", Num(0)),
            col("Buchungsdatum", Date),
            col("Belegnummer", Text),
            col("Buchungstext", Text),
            col("Kontonummer", Text),
            col("Soll", Num(2)),
            col("Haben", Num(2)),
            col_d("Steuerkennzeichen", "Kennzeichen der Steuerbuchung für die Umsatzsteuer-Voranmeldung", Text),
            col_d("Bemessungsgrundlage", "Bei Steuerbuchungen nach § 13b UStG und innergemeinschaftlichem Erwerb", Num(2)),
            col("Herkunft", Text),
            col_d("HerkunftId", "Id des Datensatzes, aus dem die Buchung entstanden ist (Rechnung, Beleg, Zahlung, Anlage, …)", Num(0)),
            col("KontaktId", Num(0)),
            col("Erfasst", Text),
            col("Festgeschrieben", Text),
        ],
        foreign_keys: vec![(&["Kontonummer"], "Konten"), (&["KontaktId"], "Kontakte")],
        rows: journal_rows,
    };

    let docs = db::query::<Document>(
        conn,
        "WHERE doc_type IN ('invoice','credit_note') AND status != 'draft' AND date >= ?1 AND date <= ?2 ORDER BY date, id",
        &range,
    )?;
    let numbers: HashMap<i64, String> = db::query::<Document>(conn, "WHERE status != 'draft'", &[])?.into_iter().map(|d| (d.id, d.number)).collect();
    let rechnungen = Table {
        file: "rechnungen.csv",
        name: "Rechnungen",
        description: "Ausgangsrechnungen, Gutschriften und Stornorechnungen".into(),
        keys: 1,
        columns: vec![
            col("RechnungId", Num(0)),
            col("Rechnungsnummer", Text),
            col("Art", Text),
            col("Rechnungsdatum", Date),
            col("Leistungsdatum", Date),
            col("LeistungBis", Date),
            col("Faellig", Date),
            col("KontaktId", Num(0)),
            col("Empfaenger", Text),
            col("Steuerart", Text),
            col("Netto", Num(2)),
            col("Steuer", Num(2)),
            col("Brutto", Num(2)),
            col("Bezahlt", Num(2)),
            col("Status", Text),
            col_d("StornoZu", "Rechnungsnummer der stornierten Rechnung", Text),
            col("Abgeschlossen", Text),
        ],
        foreign_keys: vec![(&["KontaktId"], "Kontakte")],
        rows: docs
            .iter()
            .map(|d| {
                vec![
                    d.id.to_string(),
                    d.number.clone(),
                    type_label(&d.doc_type, &d.kind).into(),
                    date(d.date),
                    date_opt(d.service_date),
                    date_opt(d.service_date_end),
                    date_opt(d.due_date),
                    id_opt(d.contact_id),
                    text(&d.address),
                    d.tax_mode.clone(),
                    money(d.net_total),
                    money(d.tax_total),
                    money(d.gross_total),
                    money(d.paid_total),
                    d.status.clone(),
                    d.cancels_id.and_then(|c| numbers.get(&c).cloned()).unwrap_or_default(),
                    d.finalized_at.clone(),
                ]
            })
            .collect(),
    };

    let mut doc_line_rows = Vec::new();
    for d in &docs {
        let doc_lines = db::query::<DocumentLine>(conn, "WHERE document_id = ?1 AND line_type IN ('item','deduction') ORDER BY position, id", &[v(d.id)])?;
        for (i, l) in doc_lines.iter().enumerate() {
            doc_line_rows.push(vec![
                d.id.to_string(),
                (i + 1).to_string(),
                if l.line_type == "deduction" { "Abzug Abschlag".into() } else { "Position".into() },
                text(&l.article_number),
                text(&l.name),
                text(&l.description),
                num(l.quantity, 4),
                text(&l.unit),
                num(l.unit_price, 4),
                money(l.discount_percent),
                money(l.tax_rate),
                l.account.clone(),
                money(l.net_amount),
                money(l.tax_amount),
                money(l.gross_amount),
            ]);
        }
    }
    let rechnungspositionen = Table {
        file: "rechnungspositionen.csv",
        name: "Rechnungspositionen",
        description: "Positionen der Ausgangsrechnungen".into(),
        keys: 2,
        columns: vec![
            col("RechnungId", Num(0)),
            col("Position", Num(0)),
            col("Art", Text),
            col("Artikelnummer", Text),
            col("Bezeichnung", Text),
            col("Beschreibung", Text),
            col("Menge", Num(4)),
            col("Einheit", Text),
            col("Einzelpreis", Num(4)),
            col("RabattProzent", Num(2)),
            col("Steuersatz", Num(2)),
            col_d("Erloeskonto", "Leer: Erlöskonto nach Steuersatz und Steuerart", Text),
            col("Netto", Num(2)),
            col("Steuer", Num(2)),
            col("Brutto", Num(2)),
        ],
        foreign_keys: vec![(&["RechnungId"], "Rechnungen")],
        rows: doc_line_rows,
    };

    let receipts = db::query::<Receipt>(conn, "WHERE date >= ?1 AND date <= ?2 ORDER BY date, id", &range)?;
    let files: HashMap<i64, String> = db::query::<FileMeta>(conn, "", &[])?.into_iter().map(|f| (f.id, f.filename)).collect();
    let belege = Table {
        file: "belege.csv",
        name: "Belege",
        description: "Eingangsrechnungen und sonstige Belege".into(),
        keys: 1,
        columns: vec![
            col("BelegId", Num(0)),
            col("Belegnummer", Text),
            col("Art", Text),
            col("Rechnungsnummer", Text),
            col("Belegdatum", Date),
            col("Leistungsdatum", Date),
            col("Faellig", Date),
            col("KontaktId", Num(0)),
            col("Lieferant", Text),
            col("Beschreibung", Text),
            col("Steuerart", Text),
            col("Netto", Num(2)),
            col("Steuer", Num(2)),
            col("Brutto", Num(2)),
            col("Bezahlt", Num(2)),
            col("Status", Text),
            col_d("Herkunft", "manual, xrechnung oder zugferd", Text),
            col("Datei", Text),
            col("Erfasst", Text),
        ],
        foreign_keys: vec![(&["KontaktId"], "Kontakte")],
        rows: receipts
            .iter()
            .map(|r| {
                vec![
                    r.id.to_string(),
                    r.number.clone(),
                    if r.receipt_type == "income" { "Einnahme".into() } else { "Ausgabe".into() },
                    text(&r.invoice_number),
                    date(r.date),
                    date_opt(r.service_date),
                    date_opt(r.due_date),
                    id_opt(r.contact_id),
                    text(&r.supplier_name),
                    text(&r.description),
                    r.tax_mode.clone(),
                    money(r.net_total),
                    money(r.tax_total),
                    money(r.gross_total),
                    money(r.paid_total),
                    r.status.clone(),
                    r.source.clone(),
                    r.file_id.and_then(|f| files.get(&f)).map(|f| text(f)).unwrap_or_default(),
                    r.created_at.clone(),
                ]
            })
            .collect(),
    };

    let mut receipt_line_rows = Vec::new();
    for r in &receipts {
        for (i, l) in db::query::<ReceiptLine>(conn, "WHERE receipt_id = ?1 ORDER BY position, id", &[v(r.id)])?.iter().enumerate() {
            receipt_line_rows.push(vec![r.id.to_string(), (i + 1).to_string(), l.account.clone(), text(&l.description), money(l.net), money(l.tax_rate), money(l.tax), money(l.gross)]);
        }
    }
    let belegpositionen = Table {
        file: "belegpositionen.csv",
        name: "Belegpositionen",
        description: "Aufteilung der Belege auf Konten und Steuersätze".into(),
        keys: 2,
        columns: vec![
            col("BelegId", Num(0)),
            col("Position", Num(0)),
            col("Kontonummer", Text),
            col("Beschreibung", Text),
            col("Netto", Num(2)),
            col("Steuersatz", Num(2)),
            col("Steuer", Num(2)),
            col("Brutto", Num(2)),
        ],
        foreign_keys: vec![(&["BelegId"], "Belege"), (&["Kontonummer"], "Konten")],
        rows: receipt_line_rows,
    };

    let payment_accounts: HashMap<i64, PaymentAccount> = db::query::<PaymentAccount>(conn, "", &[])?.into_iter().map(|a| (a.id, a)).collect();
    let transactions = db::query::<Transaction>(conn, "WHERE date >= ?1 AND date <= ?2 ORDER BY account_id, date, id", &range)?;
    let zahlungen = Table {
        file: "zahlungen.csv",
        name: "Zahlungen",
        description: "Kontobewegungen und Kassenbuch".into(),
        keys: 1,
        columns: vec![
            col("ZahlungId", Num(0)),
            col("Zahlungskonto", Text),
            col_d("Kontoart", "bank, cash (Kasse) oder other", Text),
            col("Kontonummer", Text),
            col("Datum", Date),
            col_d("Betrag", "Eingang positiv, Ausgang negativ", Num(2)),
            col("Gegenpartei", Text),
            col("GegenparteiIBAN", Text),
            col("Verwendungszweck", Text),
            col("Kassenbelegnummer", Num(0)),
            col("Status", Text),
            col_d("StornoZu", "ZahlungId der stornierten Kassenbuchung", Num(0)),
            col_d("Herkunft", "manual, Bankimport (csv, camt) oder Kassenbuch", Text),
            col("Erfasst", Text),
        ],
        foreign_keys: vec![(&["Kontonummer"], "Konten")],
        rows: transactions
            .iter()
            .map(|t| {
                let acc = payment_accounts.get(&t.account_id);
                vec![
                    t.id.to_string(),
                    acc.map(|a| text(&a.name)).unwrap_or_default(),
                    acc.map(|a| a.kind.clone()).unwrap_or_default(),
                    acc.map(|a| a.ledger_account.clone()).unwrap_or_default(),
                    date(t.date),
                    money(t.amount),
                    text(&t.counterparty),
                    t.counterparty_iban.clone(),
                    text(&t.purpose),
                    id_opt(t.cash_number),
                    t.status.clone(),
                    id_opt(t.reversal_of),
                    t.source.clone(),
                    t.created_at.clone(),
                ]
            })
            .collect(),
    };

    let assets = db::query::<Asset>(conn, "WHERE acquisition_date <= ?2 AND (disposal_date IS NULL OR disposal_date >= ?1) ORDER BY acquisition_date, id", &range)?;
    let anlagen = Table {
        file: "anlagen.csv",
        name: "Anlagen",
        description: "Anlagenverzeichnis".into(),
        keys: 1,
        columns: vec![
            col("AnlageId", Num(0)),
            col("Inventarnummer", Text),
            col("Bezeichnung", Text),
            col("Kontonummer", Text),
            col("Anschaffungsdatum", Date),
            col("Anschaffungskosten", Num(2)),
            col("NutzungsdauerJahre", Num(0)),
            col_d("Methode", "linear, gwg (geringwertiges Wirtschaftsgut), pool (Sammelposten) oder none", Text),
            col("Abgangsdatum", Date),
            col("Veraeusserungserloes", Num(2)),
            col("BelegId", Num(0)),
        ],
        // No foreign key to Belege: assets from earlier years refer to receipts outside the period.
        foreign_keys: vec![(&["Kontonummer"], "Konten")],
        rows: assets
            .iter()
            .map(|a| {
                vec![
                    a.id.to_string(),
                    a.number.clone(),
                    text(&a.name),
                    a.account.clone(),
                    date(a.acquisition_date),
                    money(a.cost),
                    a.useful_life_years.to_string(),
                    a.method.clone(),
                    date_opt(a.disposal_date),
                    money(a.disposal_proceeds),
                    id_opt(a.receipt_id),
                ]
            })
            .collect(),
    };

    // Changes logged during the period (GoBD: Nachvollziehbarkeit).
    let activities = db::query::<Activity>(conn, "WHERE substr(created_at, 1, 10) >= ?1 AND substr(created_at, 1, 10) <= ?2 ORDER BY id", &range)?;
    let protokoll = Table {
        file: "protokoll.csv",
        name: "Protokoll",
        description: "Änderungsprotokoll (Abschluss, Versand, Zahlungen, Stornos, Änderungen)".into(),
        keys: 1,
        columns: vec![
            col("ProtokollId", Num(0)),
            col("Zeitpunkt", Text),
            col_d("Objekt", "document (Rechnung), receipt (Beleg), transaction (Zahlung), …", Text),
            col("ObjektId", Num(0)),
            col("Aktion", Text),
            col("Meldung", Text),
        ],
        foreign_keys: vec![],
        rows: activities.iter().map(|a| vec![a.id.to_string(), a.created_at.clone(), a.entity.clone(), a.entity_id.to_string(), a.action.clone(), text(&a.message)]).collect(),
    };

    Ok(vec![konten, kontakte, buchungen, rechnungen, rechnungspositionen, belege, belegpositionen, zahlungen, anlagen, protokoll])
}

fn index_xml(conn: &Connection, tables: &[Table], from: NaiveDate, to: NaiveDate) -> Result<String> {
    let s = cfg::load(conn)?;
    let c = &s.company;
    let location = [format!("{} {}", c.zip, c.city).trim().to_string(), c.country.clone()].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(", ");
    let comment = format!(
        "Datenträgerüberlassung nach § 147 Abs. 6 AO für den Zeitraum {} bis {}. Erstellt am {} mit Sorobo {}.",
        date(from),
        date(to),
        now_str(),
        env!("CARGO_PKG_VERSION")
    );
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str(&format!("<!DOCTYPE DataSet SYSTEM \"{DTD_NAME}\">\n"));
    x.push_str("<DataSet>\n  <Version>1.0</Version>\n");
    x.push_str(&format!(
        "  <DataSupplier>\n    <Name>{}</Name>\n    <Location>{}</Location>\n    <Comment>{}</Comment>\n  </DataSupplier>\n",
        xml_escape(&c.name),
        xml_escape(&location),
        xml_escape(&comment)
    ));
    x.push_str("  <Media>\n    <Name>Datenträger 1</Name>\n");
    for t in tables {
        x.push_str(&t.xml(from, to));
    }
    x.push_str("  </Media>\n</DataSet>\n");
    Ok(x)
}

/// ZIP with `index.xml`, the DTD and one CSV file per table for the calendar year.
pub fn export(conn: &Connection, year: i32) -> Result<Vec<u8>> {
    let (from, to) = (ymd(year, 1, 1), ymd(year, 12, 31));
    let tables = tables(conn, from, to)?;
    let index = index_xml(conn, &tables, from, to)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let now = chrono::Local::now().naive_local();
        let mut opts = zip::write::SimpleFileOptions::default();
        if let Ok(t) = zip::DateTime::from_date_and_time(now.year() as u16, now.month() as u8, now.day() as u8, now.hour() as u8, now.minute() as u8, now.second() as u8) {
            opts = opts.last_modified_time(t);
        }
        zip.start_file("index.xml", opts)?;
        zip.write_all(index.as_bytes())?;
        zip.start_file(DTD_NAME, opts)?;
        zip.write_all(DTD.as_bytes())?;
        for t in &tables {
            zip.start_file(t.file, opts)?;
            zip.write_all(t.csv().as_bytes())?;
        }
        zip.finish()?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::documents::{self, DocumentInput};
    use crate::services::finance::{self, PaymentRequest};
    use rust_decimal_macros::dec;
    use std::collections::HashSet;
    use std::io::Read;

    /// Splits a CSV record (`;`, text in double quotes).
    fn fields(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut quoted = false;
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '"' if quoted && chars.peek() == Some(&'"') => {
                    cur.push('"');
                    chars.next();
                }
                '"' => quoted = !quoted,
                ';' if !quoted => out.push(std::mem::take(&mut cur)),
                _ => cur.push(ch),
            }
        }
        out.push(cur);
        out
    }

    fn unzip(bytes: Vec<u8>) -> HashMap<String, String> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut out = HashMap::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            out.insert(f.name().unwrap().to_string(), s);
        }
        out
    }

    #[test]
    fn export_matches_index_and_references() {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        cfg::seed_test_company(&conn);
        let mut c = Contact { company: "Kunde \"Nord\" GmbH".into(), street: "Weg 1".into(), zip: "12345".into(), city: "Stadt".into(), ..Default::default() };
        let contact = db::insert(&conn, &mut c).unwrap();
        let bank = finance::save_account(&conn, PaymentAccount { name: "Bank".into(), opening_date: ymd(2026, 1, 1), ..Default::default() }).unwrap();
        let doc = Document { doc_type: "invoice".into(), contact_id: Some(contact), address: "Kunde \"Nord\" GmbH\nWeg 1\n12345 Stadt".into(), tax_mode: "standard".into(), date: ymd(2026, 3, 2), ..Default::default() };
        let line = DocumentLine { line_type: "item".into(), name: "Beratung; Teil 1".into(), quantity: dec!(1.5), unit_price: dec!(100), tax_rate: dec!(19), ..Default::default() };
        let id = documents::create(&conn, DocumentInput { doc, lines: vec![line] }).unwrap();
        documents::finalize(&conn, id).unwrap();
        finance::pay_document(&conn, id, PaymentRequest { account_id: Some(bank), amount: dec!(178.50), date: Some(ymd(2026, 3, 20)), ..Default::default() }).unwrap();

        let files = unzip(export(&conn, 2026).unwrap());
        let index = &files["index.xml"];
        assert!(index.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE DataSet SYSTEM \"gdpdu-01-09-2004.dtd\">"));
        assert!(files.contains_key(DTD_NAME));

        let tables = tables(&conn, ymd(2026, 1, 1), ymd(2026, 12, 31)).unwrap();
        let mut keys: HashMap<&str, HashSet<String>> = HashMap::new();
        for t in &tables {
            assert!(index.contains(&format!("<URL>{}</URL>", t.file)), "{} listed in index.xml", t.file);
            let csv = &files[t.file];
            assert!(csv.ends_with("\r\n"));
            let records: Vec<&str> = csv.trim_end_matches("\r\n").split("\r\n").collect();
            assert_eq!(fields(records[0]), t.columns.iter().map(|c| c.name.to_string()).collect::<Vec<_>>(), "header of {}", t.file);
            for r in &records[1..] {
                assert_eq!(fields(r).len(), t.columns.len(), "{}: {r}", t.file);
            }
            keys.insert(t.name, records[1..].iter().map(|r| fields(r)[..t.keys].join("|")).collect());
        }
        // Foreign keys point to existing rows.
        for t in &tables {
            let csv = &files[t.file];
            for r in csv.trim_end_matches("\r\n").split("\r\n").skip(1) {
                let f = fields(r);
                for (cols, target) in &t.foreign_keys {
                    let val = cols.iter().map(|c| f[t.columns.iter().position(|x| x.name == *c).unwrap()].clone()).collect::<Vec<_>>().join("|");
                    assert!(val.is_empty() || keys[target].contains(&val), "{} → {target}: {val}", t.file);
                }
            }
        }

        let invoice = files["rechnungen.csv"].lines().nth(1).unwrap().to_string();
        assert!(invoice.contains("\"Kunde \"\"Nord\"\" GmbH, Weg 1, 12345 Stadt\""), "{invoice}");
        assert!(invoice.contains(";150,00;28,50;178,50;178,50;"), "{invoice}");
        let pos = files["rechnungspositionen.csv"].lines().nth(1).unwrap().to_string();
        assert!(pos.contains("\"Beratung; Teil 1\"") && pos.contains(";1,5000;"), "{pos}");
        let journal = &files["buchungen.csv"];
        let debit: Decimal = journal.lines().skip(1).map(|r| Decimal::from_str_exact(&fields(r)[6].replace(',', ".")).unwrap()).sum();
        let credit: Decimal = journal.lines().skip(1).map(|r| Decimal::from_str_exact(&fields(r)[7].replace(',', ".")).unwrap()).sum();
        assert_eq!(debit, credit);
        assert!(debit > Decimal::ZERO);
        assert_eq!(files["zahlungen.csv"].lines().count(), 2);
        // Other years stay out.
        assert_eq!(unzip(export(&conn, 2025).unwrap())["rechnungen.csv"].lines().count(), 1);
    }
}
