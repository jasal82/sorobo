// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! SEPA credit transfers for open receipts: a payment run collects the selected receipts and
//! produces a pain.001.001.09 file for the upload in online banking. The run only records what was
//! handed to the bank; the payment itself is booked when the bank statement is imported.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Local, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bail_user;
use crate::db::{self, v};
use crate::models::{Contact, PaymentAccount, PaymentRun, PaymentRunItem, Receipt};
use crate::services::settings as cfg;
use crate::util::{bic_valid, fmt_date, fmt_money, iban_valid, normalize_iban, now_str, today};

/// SEPA countries outside the EEA: the payee's address is mandatory there (EPC rulebook).
pub(crate) const NON_EEA_SEPA: &[&str] = &["AD", "AL", "CH", "GB", "GG", "GI", "IM", "JE", "MC", "MD", "ME", "MK", "RS", "SM", "VA"];
pub(crate) const EEA: &[&str] = &[
    "AT", "BE", "BG", "CY", "CZ", "DE", "DK", "EE", "ES", "FI", "FR", "GR", "HR", "HU", "IE", "IS", "IT", "LI", "LT", "LU", "LV", "MT", "NL", "NO", "PL", "PT", "RO", "SE", "SI", "SK",
];

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TransferCandidate {
    pub receipt_id: i64,
    pub number: String,
    pub invoice_number: String,
    pub date: NaiveDate,
    pub due_date: Option<NaiveDate>,
    #[ts(type = "number")]
    pub open_amount: Decimal,
    pub name: String,
    pub iban: String,
    pub bic: String,
    /// Suggested Verwendungszweck.
    pub remittance: String,
    /// Why the receipt cannot be transferred (empty = ok).
    pub problem: String,
    /// Execution date of a payment run that already contains the receipt.
    pub pending: Option<NaiveDate>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct TransferItemInput {
    pub receipt_id: i64,
    #[ts(type = "number")]
    pub amount: Decimal,
    pub remittance: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct PaymentRunRequest {
    pub account_id: i64,
    pub execution_date: NaiveDate,
    pub items: Vec<TransferItemInput>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PaymentRunItemView {
    pub item: PaymentRunItem,
    pub receipt_number: String,
    /// The receipt has been paid in the meantime.
    pub paid: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PaymentRunView {
    pub run: PaymentRun,
    pub account_name: String,
    pub items: Vec<PaymentRunItemView>,
}

/// Text in the SEPA character set (a–z, A–Z, 0–9, / - ? : ( ) . , ' + and space): umlauts are
/// transcribed, other characters replaced.
pub fn sepa_text(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let rep: &str = match c {
            'ä' => "ae",
            'ö' => "oe",
            'ü' => "ue",
            'Ä' => "Ae",
            'Ö' => "Oe",
            'Ü' => "Ue",
            'ß' => "ss",
            '&' => "+",
            '%' => " Prozent ",
            '€' => " EUR ",
            '–' | '—' => "-",
            'à' | 'á' | 'â' | 'ã' | 'å' => "a",
            'ç' => "c",
            'è' | 'é' | 'ê' | 'ë' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ñ' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ø' => "o",
            'ù' | 'ú' | 'û' => "u",
            'ý' | 'ÿ' => "y",
            'À' | 'Á' | 'Â' | 'Ã' | 'Å' => "A",
            'Ç' => "C",
            'È' | 'É' | 'Ê' | 'Ë' => "E",
            'Ì' | 'Í' | 'Î' | 'Ï' => "I",
            'Ñ' => "N",
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ø' => "O",
            'Ù' | 'Ú' | 'Û' => "U",
            'Ý' => "Y",
            c if c.is_ascii_alphanumeric() || "/-?:().,'+ ".contains(c) => {
                out.push(c);
                continue;
            }
            c if c.is_whitespace() => " ",
            '_' | '#' | ';' | '|' | '"' => "-",
            _ => ".",
        };
        out.push_str(rep);
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max).collect::<String>().trim().to_string()
}

/// Identifier (MsgId, EndToEndId): SEPA characters without blanks, at most 35.
pub(crate) fn sepa_id(s: &str) -> String {
    sepa_text(s, 70).replace(' ', "").chars().take(35).collect()
}

pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\'', "&apos;").replace('"', "&quot;")
}

pub(crate) fn amount(d: Decimal) -> String {
    format!("{:.2}", d.round_dp(2))
}

pub(crate) fn country(iban: &str) -> &str {
    iban.get(..2).unwrap_or("")
}

struct Payee {
    name: String,
    iban: String,
    bic: String,
    street: String,
    zip: String,
    city: String,
    country: String,
}

fn payee(r: &Receipt, contact: Option<&Contact>) -> Payee {
    let iban = if r.iban.trim().is_empty() { contact.map(|c| c.iban.clone()).unwrap_or_default() } else { r.iban.clone() };
    // The contact's BIC belongs to the contact's IBAN only.
    let bic = contact.filter(|c| normalize_iban(&c.iban) == normalize_iban(&iban)).map(|c| normalize_iban(&c.bic)).unwrap_or_default();
    Payee {
        name: contact.map(|c| c.display_name()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.supplier_name.clone()),
        iban: normalize_iban(&iban),
        bic,
        street: contact.map(|c| c.street.clone()).unwrap_or_default(),
        zip: contact.map(|c| c.zip.clone()).unwrap_or_default(),
        city: contact.map(|c| c.city.clone()).unwrap_or_default(),
        country: contact.map(|c| c.country.clone()).unwrap_or_default(),
    }
}

fn problem(p: &Payee) -> String {
    if p.iban.is_empty() {
        return "Keine IBAN beim Beleg oder Lieferanten hinterlegt".into();
    }
    if !iban_valid(&p.iban) {
        return "IBAN ist ungültig".into();
    }
    let c = country(&p.iban);
    if !EEA.contains(&c) && !NON_EEA_SEPA.contains(&c) {
        return format!("IBAN aus {c} liegt nicht im SEPA-Raum");
    }
    if NON_EEA_SEPA.contains(&c) && (p.city.trim().is_empty() || p.country.trim().is_empty()) {
        return "Anschrift des Lieferanten (Ort, Land) fehlt – für Empfänger außerhalb des EWR erforderlich".into();
    }
    if sepa_text(&p.name, 70).is_empty() {
        return "Name des Empfängers fehlt".into();
    }
    String::new()
}

fn remittance(r: &Receipt) -> String {
    let what = if r.invoice_number.trim().is_empty() { format!("Beleg {}", r.number) } else { format!("Rechnung {}", r.invoice_number.trim()) };
    sepa_text(&format!("{what} vom {}", fmt_date(r.date)), 140)
}

/// Receipts still pending in a payment run: receipt id → execution date of the latest run.
fn pending(conn: &Connection) -> Result<HashMap<i64, NaiveDate>> {
    let runs: HashMap<i64, NaiveDate> = db::query::<PaymentRun>(conn, "", &[])?.into_iter().map(|r| (r.id, r.execution_date)).collect();
    let mut out: HashMap<i64, NaiveDate> = HashMap::new();
    for it in db::query::<PaymentRunItem>(conn, "", &[])? {
        if let Some(d) = runs.get(&it.run_id) {
            let e = out.entry(it.receipt_id).or_insert(*d);
            *e = (*e).max(*d);
        }
    }
    Ok(out)
}

fn contacts(conn: &Connection) -> Result<HashMap<i64, Contact>> {
    Ok(db::query::<Contact>(conn, "", &[])?.into_iter().map(|c| (c.id, c)).collect())
}

/// Open receipts that are paid out (expense receipts with an amount to pay).
pub fn candidates(conn: &Connection) -> Result<Vec<TransferCandidate>> {
    let contacts = contacts(conn)?;
    let pending = pending(conn)?;
    let mut out = Vec::new();
    for r in db::query::<Receipt>(conn, "WHERE status IN ('open','partial') ORDER BY due_date, date, id", &[])? {
        let open = r.remaining();
        if open <= Decimal::ZERO || r.money_sign() > Decimal::ZERO {
            continue;
        }
        let p = payee(&r, r.contact_id.and_then(|c| contacts.get(&c)));
        out.push(TransferCandidate {
            receipt_id: r.id,
            number: r.number.clone(),
            invoice_number: r.invoice_number.clone(),
            date: r.date,
            due_date: r.due_date,
            open_amount: open,
            problem: problem(&p),
            name: p.name,
            iban: p.iban,
            bic: p.bic,
            remittance: remittance(&r),
            pending: pending.get(&r.id).copied(),
        });
    }
    Ok(out)
}

fn postal_address(p: &Payee) -> String {
    if p.city.trim().is_empty() || p.country.trim().len() != 2 {
        return String::new();
    }
    let mut x = String::from("<PstlAdr>");
    if !p.street.trim().is_empty() {
        x.push_str(&format!("<StrtNm>{}</StrtNm>", xml_escape(&sepa_text(&p.street, 70))));
    }
    if !p.zip.trim().is_empty() {
        x.push_str(&format!("<PstCd>{}</PstCd>", xml_escape(&sepa_text(&p.zip, 16))));
    }
    x.push_str(&format!("<TwnNm>{}</TwnNm><Ctry>{}</Ctry></PstlAdr>", xml_escape(&sepa_text(&p.city, 35)), p.country.trim().to_uppercase()));
    x
}

struct Debtor {
    name: String,
    iban: String,
    bic: String,
}

fn pain001(msg_id: &str, created: &str, debtor: &Debtor, execution: NaiveDate, items: &[(PaymentRunItem, Payee)]) -> String {
    let total: Decimal = items.iter().map(|(i, _)| i.amount).sum();
    let n = items.len();
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str("<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.001.001.09\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n");
    x.push_str("  <CstmrCdtTrfInitn>\n");
    x.push_str(&format!(
        "    <GrpHdr>\n      <MsgId>{msg_id}</MsgId>\n      <CreDtTm>{created}</CreDtTm>\n      <NbOfTxs>{n}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n      <InitgPty>\n        <Nm>{}</Nm>\n      </InitgPty>\n    </GrpHdr>\n",
        amount(total),
        xml_escape(&debtor.name)
    ));
    x.push_str("    <PmtInf>\n");
    x.push_str(&format!("      <PmtInfId>{}</PmtInfId>\n      <PmtMtd>TRF</PmtMtd>\n", sepa_id(&format!("{msg_id}-1"))));
    // Single bookings: every transfer appears on the statement and is matched to its receipt.
    x.push_str(&format!("      <BtchBookg>false</BtchBookg>\n      <NbOfTxs>{n}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n", amount(total)));
    x.push_str("      <PmtTpInf>\n        <SvcLvl>\n          <Cd>SEPA</Cd>\n        </SvcLvl>\n      </PmtTpInf>\n");
    x.push_str(&format!("      <ReqdExctnDt>\n        <Dt>{execution}</Dt>\n      </ReqdExctnDt>\n"));
    x.push_str(&format!("      <Dbtr>\n        <Nm>{}</Nm>\n      </Dbtr>\n", xml_escape(&debtor.name)));
    x.push_str(&format!("      <DbtrAcct>\n        <Id>\n          <IBAN>{}</IBAN>\n        </Id>\n      </DbtrAcct>\n", debtor.iban));
    if debtor.bic.is_empty() {
        x.push_str("      <DbtrAgt>\n        <FinInstnId>\n          <Othr>\n            <Id>NOTPROVIDED</Id>\n          </Othr>\n        </FinInstnId>\n      </DbtrAgt>\n");
    } else {
        x.push_str(&format!("      <DbtrAgt>\n        <FinInstnId>\n          <BICFI>{}</BICFI>\n        </FinInstnId>\n      </DbtrAgt>\n", debtor.bic));
    }
    x.push_str("      <ChrgBr>SLEV</ChrgBr>\n");
    for (it, p) in items {
        x.push_str("      <CdtTrfTxInf>\n");
        x.push_str(&format!("        <PmtId>\n          <EndToEndId>{}</EndToEndId>\n        </PmtId>\n", it.end_to_end_id));
        x.push_str(&format!("        <Amt>\n          <InstdAmt Ccy=\"EUR\">{}</InstdAmt>\n        </Amt>\n", amount(it.amount)));
        if bic_valid(&it.creditor_bic) {
            x.push_str(&format!("        <CdtrAgt>\n          <FinInstnId>\n            <BICFI>{}</BICFI>\n          </FinInstnId>\n        </CdtrAgt>\n", it.creditor_bic));
        }
        x.push_str(&format!("        <Cdtr>\n          <Nm>{}</Nm>\n", xml_escape(&it.creditor_name)));
        let addr = postal_address(p);
        if !addr.is_empty() {
            x.push_str(&format!("          {addr}\n"));
        }
        x.push_str("        </Cdtr>\n");
        x.push_str(&format!("        <CdtrAcct>\n          <Id>\n            <IBAN>{}</IBAN>\n          </Id>\n        </CdtrAcct>\n", it.creditor_iban));
        if !it.remittance.is_empty() {
            x.push_str(&format!("        <RmtInf>\n          <Ustrd>{}</Ustrd>\n        </RmtInf>\n", xml_escape(&it.remittance)));
        }
        x.push_str("      </CdtTrfTxInf>\n");
    }
    x.push_str("    </PmtInf>\n  </CstmrCdtTrfInitn>\n</Document>\n");
    x
}

/// Creates a payment run with its pain.001 file.
pub fn create(conn: &Connection, req: PaymentRunRequest) -> Result<PaymentRunView> {
    let s = cfg::load(conn)?;
    let Some(account) = db::find::<PaymentAccount>(conn, req.account_id)? else {
        bail_user!("Bitte ein Bankkonto auswählen.");
    };
    if account.kind != "bank" || !iban_valid(&account.iban) {
        bail_user!("Das Konto „{}“ hat keine gültige IBAN (Bank & Zahlungen → Konto bearbeiten).", account.name);
    }
    let debtor = Debtor {
        name: sepa_text(&s.company.name, 70),
        iban: normalize_iban(&account.iban),
        bic: if bic_valid(&account.bic) { normalize_iban(&account.bic) } else { String::new() },
    };
    if debtor.name.is_empty() {
        bail_user!("Bitte den Namen Ihres Unternehmens angeben (Einstellungen → Unternehmen).");
    }
    if req.execution_date < today() {
        bail_user!("Das Ausführungsdatum darf nicht in der Vergangenheit liegen.");
    }
    if req.items.is_empty() {
        bail_user!("Bitte mindestens einen Beleg auswählen.");
    }
    let contacts = contacts(conn)?;
    let mut items = Vec::new();
    for (i, input) in req.items.iter().enumerate() {
        let Some(r) = db::find::<Receipt>(conn, input.receipt_id)? else {
            bail_user!("Beleg {} nicht gefunden.", input.receipt_id);
        };
        if req.items[..i].iter().any(|x| x.receipt_id == r.id) {
            bail_user!("Beleg {} ist doppelt ausgewählt.", r.number);
        }
        let open = r.remaining();
        if r.money_sign() > Decimal::ZERO || open <= Decimal::ZERO {
            bail_user!("Beleg {} ist nicht offen bzw. keine Ausgabe.", r.number);
        }
        if input.amount <= Decimal::ZERO || input.amount.round_dp(2) != input.amount {
            bail_user!("Beleg {}: Bitte einen Betrag größer 0 mit höchstens zwei Nachkommastellen angeben.", r.number);
        }
        if input.amount > open {
            bail_user!("Beleg {}: Der Betrag ({}) ist höher als der offene Betrag ({}).", r.number, fmt_money(input.amount), fmt_money(open));
        }
        let p = payee(&r, r.contact_id.and_then(|c| contacts.get(&c)));
        let issue = problem(&p);
        if !issue.is_empty() {
            bail_user!("Beleg {}: {issue}.", r.number);
        }
        let text = if input.remittance.trim().is_empty() { remittance(&r) } else { sepa_text(&input.remittance, 140) };
        let item = PaymentRunItem {
            id: 0,
            run_id: 0,
            receipt_id: r.id,
            amount: input.amount,
            creditor_name: sepa_text(&p.name, 70),
            creditor_iban: p.iban.clone(),
            creditor_bic: if bic_valid(&p.bic) { p.bic.clone() } else { String::new() },
            remittance: text,
            end_to_end_id: String::new(),
        };
        items.push((item, p, r));
    }
    let now = Local::now();
    let mut run = PaymentRun {
        id: 0,
        account_id: account.id,
        execution_date: req.execution_date,
        message_id: String::new(),
        total: items.iter().map(|(i, _, _)| i.amount).sum(),
        count: items.len() as i64,
        created_at: now_str(),
    };
    let id = db::insert(conn, &mut run)?;
    run.message_id = sepa_id(&format!("SOROBO-{}-{id}", now.format("%Y%m%d%H%M%S")));
    db::update(conn, &run)?;
    let mut with_payee = Vec::new();
    for (mut item, p, r) in items {
        item.run_id = id;
        item.end_to_end_id = sepa_id(&format!("SO{id}-{}", r.number));
        db::insert(conn, &mut item)?;
        cfg::log(conn, "receipt", r.id, "transfer", format!("Überweisung über {} zur Ausführung am {} erstellt", fmt_money(item.amount), fmt_date(req.execution_date)))?;
        with_payee.push((item, p));
    }
    let xml = pain001(&run.message_id, &now.format("%Y-%m-%dT%H:%M:%S").to_string(), &debtor, req.execution_date, &with_payee);
    conn.execute("UPDATE payment_runs SET xml = ?1 WHERE id = ?2", rusqlite::params![xml, id])?;
    get(conn, id)
}

pub fn get(conn: &Connection, id: i64) -> Result<PaymentRunView> {
    let run: PaymentRun = db::get(conn, id)?;
    let account_name = db::find::<PaymentAccount>(conn, run.account_id)?.map(|a| a.name).unwrap_or_default();
    let mut items = Vec::new();
    for item in db::query::<PaymentRunItem>(conn, "WHERE run_id = ?1 ORDER BY id", &[v(id)])? {
        let r = db::find::<Receipt>(conn, item.receipt_id)?;
        items.push(PaymentRunItemView {
            receipt_number: r.as_ref().map(|r| r.number.clone()).unwrap_or_default(),
            paid: r.is_none_or(|r| r.status == "paid"),
            item,
        });
    }
    Ok(PaymentRunView { run, account_name, items })
}

pub fn list(conn: &Connection) -> Result<Vec<PaymentRunView>> {
    db::query::<PaymentRun>(conn, "ORDER BY id DESC LIMIT 50", &[])?.into_iter().map(|r| get(conn, r.id)).collect()
}

/// (file name, pain.001 XML) of a payment run.
pub fn file(conn: &Connection, id: i64) -> Result<(String, String)> {
    let run: PaymentRun = db::get(conn, id)?;
    let xml: String = conn.query_row("SELECT xml FROM payment_runs WHERE id = ?1", [id], |r| r.get(0))?;
    Ok((format!("SEPA-Ueberweisungen_{}_{}.xml", run.execution_date, run.id), xml))
}

/// Discards a payment run (e.g. the file was not uploaded); the receipts stay open.
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let run: PaymentRun = db::get(conn, id)?;
    for item in db::query::<PaymentRunItem>(conn, "WHERE run_id = ?1", &[v(id)])? {
        cfg::log(conn, "receipt", item.receipt_id, "transfer_removed", format!("Überweisungsdatei vom {} verworfen", fmt_date(run.execution_date)))?;
    }
    conn.execute("DELETE FROM payment_run_items WHERE run_id = ?1", [id])?;
    conn.execute("DELETE FROM payment_runs WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ReceiptLine;
    use crate::services::finance;
    use crate::services::receipts::{self, ReceiptInput};
    use crate::util::ymd;
    use rust_decimal_macros::dec;

    fn setup() -> (Connection, i64) {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        cfg::seed_test_company(&conn);
        let bank = finance::save_account(
            &conn,
            PaymentAccount { name: "Bank".into(), iban: "DE89 3704 0044 0532 0130 00".into(), bic: "COBADEFFXXX".into(), opening_date: ymd(2026, 1, 1), ..Default::default() },
        )
        .unwrap();
        (conn, bank)
    }

    fn supplier(conn: &Connection, name: &str, iban: &str, city: &str, country: &str) -> Contact {
        let mut c = Contact { company: name.into(), is_supplier: true, iban: iban.into(), city: city.into(), country: country.into(), street: "Hauptstraße 1".into(), zip: "8000".into(), ..Default::default() };
        db::insert(conn, &mut c).unwrap();
        c
    }

    fn receipt(conn: &Connection, c: &Contact, nr: &str, gross: Decimal) -> i64 {
        let r = Receipt { receipt_type: "expense".into(), contact_id: Some(c.id), supplier_name: c.display_name(), invoice_number: nr.into(), date: ymd(2026, 3, 2), tax_mode: "standard".into(), prices_gross: true, ..Default::default() };
        let line = ReceiptLine { account: "4930".into(), description: "Büro".into(), gross, net: gross, tax_rate: dec!(19), ..Default::default() };
        receipts::create(conn, ReceiptInput { receipt: r, lines: vec![line], payment: None }).unwrap()
    }

    #[test]
    fn iban_and_text_rules() {
        assert!(iban_valid("DE89 3704 0044 0532 0130 00"));
        assert!(!iban_valid("DE89 3704 0044 0532 0130 01"));
        assert!(iban_valid("CH9300762011623852957"));
        assert!(bic_valid("COBADEFF") && bic_valid("COBADEFFXXX") && !bic_valid("COBA1EFF"));
        assert_eq!(sepa_text("Müller & Söhne GmbH; Straße_3 – 5€ (19%)", 140), "Mueller + Soehne GmbH- Strasse-3 - 5 EUR (19 Prozent )");
        assert_eq!(sepa_id("SO1-ER 2026/0001 ä"), "SO1-ER2026/0001ae");
        assert_eq!(sepa_text(&"x".repeat(200), 140).len(), 140);
    }

    #[test]
    fn candidates_report_problems() {
        let (conn, _) = setup();
        let ok = supplier(&conn, "Papier AG", "DE89370400440532013000", "Köln", "DE");
        let none = supplier(&conn, "Ohne IBAN", "", "Köln", "DE");
        let swiss = supplier(&conn, "Uhren SA", "CH9300762011623852957", "", "");
        let us = supplier(&conn, "Cloud Inc", "US12345678901234567", "Seattle", "US");
        receipt(&conn, &ok, "R-1", dec!(119));
        receipt(&conn, &none, "R-2", dec!(50));
        receipt(&conn, &swiss, "R-3", dec!(70));
        receipt(&conn, &us, "R-4", dec!(10));
        let c = candidates(&conn).unwrap();
        let problems: Vec<(&str, bool)> = c.iter().map(|c| (c.invoice_number.as_str(), c.problem.is_empty())).collect();
        assert_eq!(problems, vec![("R-1", true), ("R-2", false), ("R-3", false), ("R-4", false)]);
        assert_eq!(c[0].remittance, "Rechnung R-1 vom 02.03.2026");
        assert!(c[2].problem.contains("Anschrift"), "{}", c[2].problem);
    }

    #[test]
    fn creates_pain001_and_marks_pending() {
        let (conn, bank) = setup();
        let a = supplier(&conn, "Papier & Co. KG", "DE89370400440532013000", "Köln", "DE");
        let ch = supplier(&conn, "Uhren SA", "CH9300762011623852957", "Zürich", "CH");
        let r1 = receipt(&conn, &a, "R-1", dec!(119));
        let r2 = receipt(&conn, &ch, "R-2", dec!(70.5));
        let exec = today() + chrono::Duration::days(1);
        let bad = |items: Vec<TransferItemInput>| create(&conn, PaymentRunRequest { account_id: bank, execution_date: exec, items }).unwrap_err().to_string();
        assert!(bad(vec![TransferItemInput { receipt_id: r1, amount: dec!(120), remittance: String::new() }]).contains("höher als der offene Betrag"));
        assert!(bad(vec![TransferItemInput { receipt_id: r1, amount: dec!(1.001), remittance: String::new() }]).contains("zwei Nachkommastellen"));
        let past = create(&conn, PaymentRunRequest { account_id: bank, execution_date: today() - chrono::Duration::days(1), items: vec![TransferItemInput { receipt_id: r1, amount: dec!(1), remittance: String::new() }] });
        assert!(past.is_err());

        let run = create(
            &conn,
            PaymentRunRequest {
                account_id: bank,
                execution_date: exec,
                items: vec![
                    // Skonto deducted by hand.
                    TransferItemInput { receipt_id: r1, amount: dec!(116.62), remittance: "Rechnung R-1 abzgl. 2 % Skonto".into() },
                    TransferItemInput { receipt_id: r2, amount: dec!(70.5), remittance: String::new() },
                ],
            },
        )
        .unwrap();
        assert_eq!((run.run.count, run.run.total), (2, dec!(187.12)));
        let (name, xml) = file(&conn, run.run.id).unwrap();
        assert!(name.ends_with(".xml"));
        for part in [
            "xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.001.001.09\"",
            "<NbOfTxs>2</NbOfTxs>",
            "<CtrlSum>187.12</CtrlSum>",
            "<BtchBookg>false</BtchBookg>",
            &format!("<Dt>{exec}</Dt>"),
            "<IBAN>DE89370400440532013000</IBAN>",
            "<BICFI>COBADEFFXXX</BICFI>",
            "<Nm>Papier + Co. KG</Nm>",
            "<InstdAmt Ccy=\"EUR\">116.62</InstdAmt>",
            "<Ustrd>Rechnung R-1 abzgl. 2 Prozent Skonto</Ustrd>",
            "<Ustrd>Rechnung R-2 vom 02.03.2026</Ustrd>",
            "<PstlAdr><StrtNm>Hauptstrasse 1</StrtNm><PstCd>8000</PstCd><TwnNm>Zuerich</TwnNm><Ctry>CH</Ctry></PstlAdr>",
        ] {
            assert!(xml.contains(part), "missing {part}\n{xml}");
        }
        let c = candidates(&conn).unwrap();
        assert!(c.iter().all(|c| c.pending == Some(exec)));
        assert_eq!(list(&conn).unwrap().len(), 1);
        delete(&conn, run.run.id).unwrap();
        assert!(candidates(&conn).unwrap().iter().all(|c| c.pending.is_none()));
    }
}
