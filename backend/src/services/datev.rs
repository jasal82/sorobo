// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! DATEV-Export (Buchungsstapel im EXTF-Format) für die Übergabe an die
//! Steuerberatung.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Datelike, Local, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;
use rust_decimal::prelude::Signed;

use crate::chart;
use crate::db::{self, v};
use crate::models::{JournalEntry, JournalLine};
use crate::services::settings as cfg;

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\"").chars().take(60).collect::<String>())
}

/// Belegfeld 1 only allows a–z, A–Z, 0–9 and $ & % * + - / with at most 36 characters.
fn belegfeld(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || "$&%*+-/".contains(*c)).take(36).collect()
}

fn amount(d: Decimal) -> String {
    format!("{:.2}", d.abs()).replace('.', ",")
}

pub fn export(conn: &Connection, from: NaiveDate, to: NaiveDate, advisor: &str, client: &str) -> Result<Vec<u8>> {
    // A Buchungsstapel belongs to one fiscal year; its Belegdatum carries no year.
    if from > to || from.year() != to.year() {
        crate::bail_user!("Der DATEV-Export ist nur innerhalb eines Wirtschaftsjahres möglich. Bitte einen Zeitraum innerhalb eines Kalenderjahres wählen.");
    }
    let s = cfg::load(conn)?;
    let accounts: HashMap<String, crate::models::Account> = cfg::accounts(conn)?.into_iter().map(|a| (a.number.clone(), a)).collect();
    let skr = if s.tax.chart == "SKR04" { "04" } else { "03" };
    let role = |r: &str| chart::role(&s.tax.chart, r);
    let (vst19, vst7, ust19, ust7) = (role("vst19"), role("vst7"), role("ust19"), role("ust7"));
    let mut out = String::new();
    let now = Local::now();
    out.push_str(&format!(
        "\"EXTF\";700;21;\"Buchungsstapel\";13;{};;\"SO\";\"Sorobo\";\"\";{};{};{};4;{};{};\"Sorobo Buchungsstapel\";\"\";1;0;0;\"EUR\";;\"\";;;\"{}\";;;\"\";\"\"\r\n",
        now.format("%Y%m%d%H%M%S000"),
        if advisor.is_empty() { "1001" } else { advisor },
        if client.is_empty() { "1" } else { client },
        format!("{}0101", from.year()),
        from.format("%Y%m%d"),
        to.format("%Y%m%d"),
        skr
    ));
    out.push_str("Umsatz (ohne Soll/Haben-Kz);Soll/Haben-Kennzeichen;WKZ Umsatz;Kurs;Basis-Umsatz;WKZ Basis-Umsatz;Konto;Gegenkonto (ohne BU-Schlüssel);BU-Schlüssel;Belegdatum;Belegfeld 1;Belegfeld 2;Skonto;Buchungstext\r\n");
    let entries = db::query::<JournalEntry>(conn, "WHERE date >= ?1 AND date <= ?2 ORDER BY date, id", &[v(from), v(to)])?;
    for e in entries {
        let lines = db::query::<JournalLine>(conn, "WHERE entry_id = ?1 ORDER BY id", &[v(e.id)])?;
        // (account, signed amount (+ = Soll), BU key)
        let mut items: Vec<(String, Decimal, String, String)> =
            lines.iter().map(|l| (l.account.clone(), l.debit - l.credit, String::new(), l.tax_code.clone())).collect();
        // Merge standard VAT lines into their base line (gross booking with BU key / Automatikkonto).
        let tax_idx: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, (acc, _, _, code))| (code == "VST" && (*acc == vst19 || *acc == vst7)) || ((code == "UST19" || code == "UST7") && (*acc == ust19 || *acc == ust7)))
            .map(|(i, _)| i)
            .collect();
        let mut remove = Vec::new();
        for ti in tax_idx {
            let (tacc, tamt, _, tcode) = items[ti].clone();
            let rate = if tacc == vst19 || tacc == ust19 { 19 } else { 7 };
            let base_line = lines.get(ti).map(|l| l.base).unwrap_or_default();
            let target = items.iter().position(|(acc, amt, _, code)| {
                *acc != tacc && !code.starts_with("UST") && code != "VST" && amt.signum() == tamt.signum() && (base_line.is_zero() || (*amt - base_line).abs() < Decimal::new(2, 2))
            });
            if let Some(bi) = target {
                items[bi].1 += tamt;
                let auto = accounts.get(&items[bi].0).map(|a| a.datev_auto).unwrap_or(false);
                if !auto {
                    items[bi].2 = match (tcode.as_str(), rate) {
                        ("VST", 19) => "9".into(),
                        ("VST", _) => "8".into(),
                        (_, 19) => "3".into(),
                        _ => "2".into(),
                    };
                }
                remove.push(ti);
            }
        }
        let items: Vec<_> = items.into_iter().enumerate().filter(|(i, _)| !remove.contains(i)).map(|(_, x)| x).collect();
        if items.len() < 2 {
            continue;
        }
        // The BU key belongs to the Gegenkonto, so the main account (Konto) is the largest line
        // without a key; on equal amounts a balance account (bank, receivables, …) is preferred.
        let is_balance = |acc: &str| accounts.get(acc).is_some_and(|a| a.kind != "revenue" && a.kind != "expense");
        let main = items
            .iter()
            .enumerate()
            .filter(|(_, (_, _, bu, _))| bu.is_empty())
            .max_by_key(|(_, (acc, amt, _, _))| (amt.abs(), is_balance(acc)))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let main_acc = items[main].0.clone();
        for (i, (acc, amt, bu, _)) in items.iter().enumerate() {
            if i == main || amt.is_zero() {
                continue;
            }
            // The main account is debited when the counter line is a credit.
            let sh = if *amt < Decimal::ZERO { "S" } else { "H" };
            out.push_str(&format!(
                "{};\"{}\";\"EUR\";;;;{};{};{};{};{};;;{}\r\n",
                amount(*amt),
                sh,
                main_acc,
                acc,
                if bu.is_empty() { String::new() } else { q(bu) },
                e.date.format("%d%m"),
                q(&belegfeld(&e.doc_ref)),
                q(&e.description)
            ));
        }
    }
    // DATEV expects Windows-1252
    let safe = cp1252_safe(&out);
    let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode(&safe);
    Ok(bytes.into_owned())
}

/// Replaces characters that Windows-1252 cannot represent (encoding_rs would emit
/// HTML entities like `&#8594;` for them).
fn cp1252_safe(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut buf = [0u8; 4];
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        let (_, _, unmappable) = encoding_rs::WINDOWS_1252.encode(c.encode_utf8(&mut buf));
        if !unmappable {
            out.push(c);
            continue;
        }
        out.push_str(match c {
            '→' | '⇒' => "->",
            '←' | '⇐' => "<-",
            '≤' => "<=",
            '≥' => ">=",
            '\u{2009}' | '\u{202F}' => " ",
            _ => "?",
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ymd;
    use rust_decimal_macros::dec;

    /// Inserts a journal entry; lines are (account, signed amount (+ = Soll), tax code, base).
    fn entry(conn: &Connection, lines: &[(&str, Decimal, &str, Decimal)]) {
        let mut e = JournalEntry { date: ymd(2026, 3, 10), description: "Test".into(), doc_ref: "B-1".into(), source_type: "test".into(), ..Default::default() };
        let id = db::insert(conn, &mut e).unwrap();
        for (account, amount, code, base) in lines {
            let mut l = JournalLine {
                entry_id: id,
                account: account.to_string(),
                debit: if *amount > Decimal::ZERO { *amount } else { Decimal::ZERO },
                credit: if *amount < Decimal::ZERO { -*amount } else { Decimal::ZERO },
                tax_code: code.to_string(),
                base: *base,
                ..Default::default()
            };
            db::insert(conn, &mut l).unwrap();
        }
    }

    /// Booking rows as (Umsatz, S/H, Konto, Gegenkonto, BU).
    fn rows(conn: &Connection) -> Vec<(String, String, String, String, String)> {
        let bytes = export(conn, ymd(2026, 1, 1), ymd(2026, 12, 31), "", "").unwrap();
        let text = encoding_rs::WINDOWS_1252.decode(&bytes).0.into_owned();
        text.lines()
            .skip(2)
            .map(|l| {
                let c: Vec<String> = l.split(';').map(|s| s.trim_matches('"').to_string()).collect();
                (c[0].clone(), c[1].clone(), c[6].clone(), c[7].clone(), c[8].clone())
            })
            .collect()
    }

    fn setup() -> Connection {
        let conn = db::open_memory();
        cfg::ensure_seeded(&conn).unwrap();
        conn
    }

    fn r(amount: &str, sh: &str, konto: &str, gegen: &str, bu: &str) -> (String, String, String, String, String) {
        (amount.into(), sh.into(), konto.into(), gegen.into(), bu.into())
    }

    #[test]
    fn belegfeld_and_period_are_valid_for_datev() {
        assert_eq!(belegfeld("Kasse 12"), "Kasse12");
        assert_eq!(belegfeld("RE-2026-0001"), "RE-2026-0001");
        assert_eq!(belegfeld("Beleg_Nr.7 (Ü)"), "BelegNr7");
        assert_eq!(belegfeld(&"X".repeat(50)).len(), 36);
        let conn = setup();
        assert!(export(&conn, ymd(2025, 12, 1), ymd(2026, 1, 31), "", "").is_err(), "two fiscal years");
        assert!(export(&conn, ymd(2026, 3, 1), ymd(2026, 2, 1), "", "").is_err(), "reversed range");
    }

    #[test]
    fn expense_payment_keeps_vorsteuer_key() {
        // EÜR payment of a receipt: money line first, then expense and input tax (SKR03).
        let conn = setup();
        entry(&conn, &[("1200", dec!(-119), "", dec!(0)), ("4930", dec!(100), "", dec!(0)), ("1576", dec!(19), "VST", dec!(100))]);
        assert_eq!(rows(&conn), vec![r("119,00", "H", "1200", "4930", "9")]);
    }

    #[test]
    fn manual_booking_keeps_vorsteuer_key() {
        // Manual booking: expense line first, money line last.
        let conn = setup();
        entry(&conn, &[("4930", dec!(10), "", dec!(0)), ("1571", dec!(0.70), "VST", dec!(10)), ("1200", dec!(-10.70), "", dec!(0))]);
        assert_eq!(rows(&conn), vec![r("10,70", "H", "1200", "4930", "8")]);
    }

    #[test]
    fn revenue_keys_depend_on_automatic_accounts() {
        let conn = setup();
        // 8400 is an Automatikkonto: gross amount without BU key.
        entry(&conn, &[("1200", dec!(119), "", dec!(0)), ("8400", dec!(-100), "S19", dec!(0)), ("1776", dec!(-19), "UST19", dec!(-100))]);
        // A custom revenue account needs BU 3 (19 %) / 2 (7 %).
        let mut a = crate::models::Account { number: "8401".into(), name: "Erlöse Beratung".into(), kind: "revenue".into(), ..Default::default() };
        db::insert(&conn, &mut a).unwrap();
        entry(&conn, &[("1200", dec!(107), "", dec!(0)), ("8401", dec!(-100), "S7", dec!(0)), ("1771", dec!(-7), "UST7", dec!(-100))]);
        assert_eq!(rows(&conn), vec![r("119,00", "S", "1200", "8400", ""), r("107,00", "S", "1200", "8401", "2")]);
    }

    #[test]
    fn split_receipt_keeps_both_keys() {
        let conn = setup();
        entry(
            &conn,
            &[
                ("1200", dec!(-226), "", dec!(0)),
                ("4930", dec!(100), "", dec!(0)),
                ("1576", dec!(19), "VST", dec!(100)),
                ("4940", dec!(100), "", dec!(0)),
                ("1571", dec!(7), "VST", dec!(100)),
            ],
        );
        assert_eq!(rows(&conn), vec![r("119,00", "H", "1200", "4930", "9"), r("107,00", "H", "1200", "4940", "8")]);
    }
}
