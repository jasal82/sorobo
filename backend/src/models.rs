// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Domain models. Every struct here is exported to TypeScript (`ts-rs`) so the
//! frontend shares exactly the same types as the backend.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal::prelude::Signed;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{entity, json_field};

// ---------------------------------------------------------------------------
// Stammdaten
// ---------------------------------------------------------------------------

entity! {
    pub struct FileMeta in "files" {
        pub id: i64,
        pub uuid: String,
        pub filename: String,
        pub mime: String,
        pub size: i64,
        pub created_at: String,
    }
}

entity! {
    /// Konto im Kontenrahmen (SKR03/SKR04).
    pub struct Account in "accounts" {
        pub id: i64,
        pub number: String,
        pub name: String,
        /// asset | current | liability | equity | private | tax | revenue | expense | neutral
        pub kind: String,
        pub euer: String,
        pub guv: String,
        pub bwa: String,
        pub bilanz: String,
        #[ts(type = "number | null")]
        pub tax_rate: Option<Decimal>,
        pub tax_code: String,
        pub role: String,
        pub datev_auto: bool,
        /// Where the account is offered in the UI: expense | revenue | asset | private | tax | transfer | bank | cash | other
        pub pick: String,
        pub hint: String,
        pub system: bool,
        pub active: bool,
    }
}

entity! {
    pub struct NumberRange in "number_ranges" {
        pub id: i64,
        pub key: String,
        pub label: String,
        pub format: String,
        pub next: i64,
        pub year: i64,
        pub reset_yearly: bool,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ContactPerson {
    pub salutation: String,
    pub title: String,
    pub first_name: String,
    pub last_name: String,
    pub position: String,
    pub email: String,
    pub phone: String,
    pub primary: bool,
}

entity! {
    pub struct Contact in "contacts" {
        pub id: i64,
        pub number: String,
        pub is_customer: bool,
        pub is_supplier: bool,
        /// organization | person
        pub kind: String,
        pub company: String,
        pub salutation: String,
        pub title: String,
        pub first_name: String,
        pub last_name: String,
        pub address_extra: String,
        pub street: String,
        pub zip: String,
        pub city: String,
        pub country: String,
        pub email: String,
        pub phone: String,
        pub mobile: String,
        pub website: String,
        pub vat_id: String,
        pub tax_number: String,
        pub leitweg_id: String,
        pub iban: String,
        pub bic: String,
        pub payment_terms_days: Option<i64>,
        #[ts(type = "number | null")]
        pub discount_percent: Option<Decimal>,
        /// pdf | zugferd | xrechnung
        pub einvoice_format: String,
        pub default_tax_mode: String,
        pub persons: Vec<ContactPerson>,
        pub notes: String,
        pub tags: Vec<String>,
        pub archived: bool,
        pub created_at: String,
        pub updated_at: String,
    }
}

impl Contact {
    pub fn display_name(&self) -> String {
        if self.kind == "person" || self.company.trim().is_empty() {
            let n = format!("{} {}", self.first_name.trim(), self.last_name.trim()).trim().to_string();
            if n.is_empty() { self.company.clone() } else { n }
        } else {
            self.company.clone()
        }
    }

    pub fn primary_person(&self) -> Option<&ContactPerson> {
        self.persons.iter().find(|p| p.primary).or(self.persons.first())
    }

    /// Multi-line postal address as printed in the address window.
    pub fn address_block(&self, person: Option<&str>, home_country: &str) -> String {
        let mut lines: Vec<String> = Vec::new();
        if self.kind == "person" || self.company.trim().is_empty() {
            let name = [self.salutation.as_str(), self.title.as_str(), self.first_name.as_str(), self.last_name.as_str()]
                .iter()
                .filter(|s| !s.trim().is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            lines.push(name);
        } else {
            lines.push(self.company.clone());
            if let Some(p) = person.filter(|p| !p.trim().is_empty()) {
                lines.push(p.to_string());
            }
        }
        if !self.address_extra.trim().is_empty() {
            lines.push(self.address_extra.clone());
        }
        if !self.street.trim().is_empty() {
            lines.push(self.street.clone());
        }
        let city = format!("{} {}", self.zip.trim(), self.city.trim()).trim().to_string();
        if !city.is_empty() {
            lines.push(city);
        }
        if !self.country.is_empty() && self.country != home_country {
            lines.push(crate::util::country_name(&self.country).to_uppercase());
        }
        lines.retain(|l| !l.trim().is_empty());
        lines.join("\n")
    }
}

entity! {
    /// Produkt oder Dienstleistung.
    pub struct Article in "articles" {
        pub id: i64,
        pub number: String,
        /// product | service
        pub kind: String,
        pub name: String,
        pub description: String,
        pub unit: String,
        #[ts(type = "number")]
        pub price: Decimal,
        #[ts(type = "number")]
        pub purchase_price: Decimal,
        #[ts(type = "number")]
        pub tax_rate: Decimal,
        pub revenue_account: String,
        pub category: String,
        pub gtin: String,
        pub track_stock: bool,
        #[ts(type = "number")]
        pub stock: Decimal,
        pub archived: bool,
        pub created_at: String,
        pub updated_at: String,
    }
}

// ---------------------------------------------------------------------------
// Layouts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct LayoutConfig {
    pub font: String,
    pub font_size: f64,
    pub accent_color: String,
    pub text_color: String,
    pub margin_top: f64,
    pub margin_bottom: f64,
    pub margin_left: f64,
    pub margin_right: f64,
    /// Start of the body text on the first page (mm from top).
    pub content_top: f64,
    pub logo_file_id: Option<i64>,
    pub logo_x: f64,
    pub logo_y: f64,
    pub logo_width: f64,
    pub logo_all_pages: bool,
    pub letterhead_first_id: Option<i64>,
    pub letterhead_other_id: Option<i64>,
    pub show_sender_line: bool,
    pub sender_line: String,
    pub address_x: f64,
    pub address_y: f64,
    pub address_width: f64,
    pub address_height: f64,
    pub info_x: f64,
    pub info_y: f64,
    pub info_width: f64,
    pub show_company_block: bool,
    pub company_x: f64,
    pub company_y: f64,
    pub company_width: f64,
    pub company_align: String,
    pub title_size: f64,
    /// lines | striped | grid | minimal
    pub table_style: String,
    pub table_header_fill: bool,
    pub show_position: bool,
    pub show_article_number: bool,
    pub show_unit_column: bool,
    pub show_tax_column: bool,
    pub show_footer: bool,
    /// auto | custom
    pub footer_mode: String,
    pub footer_custom: Vec<String>,
    pub footer_size: f64,
    pub show_page_numbers: bool,
    pub fold_marks: bool,
    pub show_girocode: bool,
    pub accent_bar: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            font: "Inter".into(),
            font_size: 9.5,
            accent_color: "#1d4ed8".into(),
            text_color: "#1f2937".into(),
            margin_top: 25.0,
            margin_bottom: 32.0,
            margin_left: 25.0,
            margin_right: 20.0,
            content_top: 103.0,
            logo_file_id: None,
            logo_x: 140.0,
            logo_y: 14.0,
            logo_width: 50.0,
            logo_all_pages: false,
            letterhead_first_id: None,
            letterhead_other_id: None,
            show_sender_line: true,
            sender_line: String::new(),
            address_x: 25.0,
            address_y: 48.0,
            address_width: 85.0,
            address_height: 40.0,
            info_x: 130.0,
            info_y: 52.0,
            info_width: 60.0,
            show_company_block: false,
            company_x: 130.0,
            company_y: 14.0,
            company_width: 60.0,
            company_align: "right".into(),
            title_size: 15.0,
            table_style: "lines".into(),
            table_header_fill: true,
            show_position: true,
            show_article_number: false,
            show_unit_column: true,
            show_tax_column: false,
            show_footer: true,
            footer_mode: "auto".into(),
            footer_custom: vec![],
            footer_size: 6.5,
            show_page_numbers: true,
            fold_marks: true,
            show_girocode: true,
            accent_bar: false,
        }
    }
}
json_field!(LayoutConfig);

entity! {
    pub struct Layout in "layouts" {
        pub id: i64,
        pub name: String,
        pub is_default: bool,
        pub config: LayoutConfig,
        pub created_at: String,
        pub updated_at: String,
    }
}

// ---------------------------------------------------------------------------
// Verkaufsdokumente
// ---------------------------------------------------------------------------

entity! {
    /// Angebot, Auftragsbestätigung, Lieferschein, Rechnung oder Gutschrift.
    pub struct Document in "documents" {
        pub id: i64,
        /// quote | order | delivery | invoice | credit_note
        pub doc_type: String,
        /// standard | partial | final | cancellation
        pub kind: String,
        /// draft | open | accepted | rejected | done | partial | paid | cancelled
        pub status: String,
        pub number: String,
        pub contact_id: Option<i64>,
        pub address: String,
        pub contact_person: String,
        pub date: NaiveDate,
        pub service_date: Option<NaiveDate>,
        pub service_date_end: Option<NaiveDate>,
        pub due_date: Option<NaiveDate>,
        pub payment_terms_days: Option<i64>,
        pub valid_until: Option<NaiveDate>,
        pub title: String,
        pub reference: String,
        pub buyer_reference: String,
        pub intro_text: String,
        pub closing_text: String,
        /// standard | small_business | reverse_charge | eu_service | eu_delivery | export | third_country
        pub tax_mode: String,
        pub prices_gross: bool,
        #[ts(type = "number")]
        pub cash_discount_percent: Decimal,
        pub cash_discount_days: i64,
        pub layout_id: Option<i64>,
        pub currency: String,
        #[ts(type = "number")]
        pub net_total: Decimal,
        #[ts(type = "number")]
        pub tax_total: Decimal,
        #[ts(type = "number")]
        pub gross_total: Decimal,
        #[ts(type = "number")]
        pub paid_total: Decimal,
        pub source_id: Option<i64>,
        pub base_id: Option<i64>,
        pub partial_mode: String,
        #[ts(type = "number")]
        pub partial_value: Decimal,
        pub cancels_id: Option<i64>,
        pub einvoice_format: String,
        pub dunning_level: i64,
        pub finalized_at: String,
        pub sent_at: String,
        pub paid_at: Option<NaiveDate>,
        pub notes: String,
        pub created_at: String,
        pub updated_at: String,
        /// "" (transfer) | debit (SEPA-Lastschrift)
        pub payment_method: String,
        /// Lastschriftmandat used to collect the invoice.
        pub mandate_id: Option<i64>,
    }
}

impl Document {
    pub fn is_invoice_like(&self) -> bool {
        self.doc_type == "invoice" || self.doc_type == "credit_note"
    }
    /// Accounting sign: +1 for invoices (receivable), -1 for credit notes.
    pub fn sign(&self) -> Decimal {
        if self.doc_type == "credit_note" { Decimal::NEGATIVE_ONE } else { Decimal::ONE }
    }
    pub fn open_amount(&self) -> Decimal {
        if self.status == "draft" || self.status == "cancelled" || !self.is_invoice_like() {
            return Decimal::ZERO;
        }
        self.gross_total - self.paid_total
    }
    /// Direction of the money that settles the document: +1 if it is received, -1 if it is paid
    /// out (credit notes, invoices with a negative total – e.g. a Schlussrechnung below its Abschläge).
    pub fn money_sign(&self) -> Decimal {
        self.sign() * self.gross_total.signum()
    }
    /// Amount still to be paid or refunded – positive while the document is open.
    pub fn remaining(&self) -> Decimal {
        self.open_amount() * self.gross_total.signum()
    }
}

entity! {
    pub struct DocumentLine in "document_lines" {
        pub id: i64,
        pub document_id: i64,
        pub position: i64,
        /// item | text | subtotal | deduction | pagebreak
        pub line_type: String,
        pub article_id: Option<i64>,
        pub article_number: String,
        pub name: String,
        pub description: String,
        #[ts(type = "number")]
        pub quantity: Decimal,
        pub unit: String,
        #[ts(type = "number")]
        pub unit_price: Decimal,
        #[ts(type = "number")]
        pub discount_percent: Decimal,
        #[ts(type = "number")]
        pub tax_rate: Decimal,
        pub account: String,
        #[ts(type = "number")]
        pub net_amount: Decimal,
        #[ts(type = "number")]
        pub tax_amount: Decimal,
        #[ts(type = "number")]
        pub gross_amount: Decimal,
        pub ref_document_id: Option<i64>,
    }
}

// ---------------------------------------------------------------------------
// Zahlungsverkehr
// ---------------------------------------------------------------------------

entity! {
    pub struct PaymentAccount in "payment_accounts" {
        pub id: i64,
        pub name: String,
        /// bank | cash | other
        pub kind: String,
        pub iban: String,
        pub bic: String,
        pub bank_name: String,
        pub ledger_account: String,
        #[ts(type = "number")]
        pub opening_balance: Decimal,
        pub opening_date: NaiveDate,
        pub color: String,
        pub archived: bool,
        pub created_at: String,
    }
}

entity! {
    /// Kontobewegung (Bank) oder Kassenbuchung.
    pub struct Transaction in "transactions" {
        pub id: i64,
        pub account_id: i64,
        pub date: NaiveDate,
        /// Positive = Eingang, negativ = Ausgang.
        #[ts(type = "number")]
        pub amount: Decimal,
        pub counterparty: String,
        pub counterparty_iban: String,
        pub purpose: String,
        pub cash_number: Option<i64>,
        /// open | booked
        pub status: String,
        pub import_hash: String,
        pub source: String,
        pub reversal_of: Option<i64>,
        pub created_at: String,
    }
}

entity! {
    /// Zuordnung einer Zahlung (oder Ausbuchung) zu Rechnung, Beleg oder Konto.
    pub struct Allocation in "allocations" {
        pub id: i64,
        pub transaction_id: Option<i64>,
        /// document | receipt | account | writeoff
        pub kind: String,
        pub document_id: Option<i64>,
        pub receipt_id: Option<i64>,
        pub account: String,
        #[ts(type = "number")]
        pub tax_rate: Decimal,
        /// Signed like the transaction (incoming positive).
        #[ts(type = "number")]
        pub amount: Decimal,
        pub date: NaiveDate,
        /// skonto | loss | other
        pub writeoff_reason: String,
        pub description: String,
        pub created_at: String,
    }
}

// ---------------------------------------------------------------------------
// Belege
// ---------------------------------------------------------------------------

entity! {
    pub struct Receipt in "receipts" {
        pub id: i64,
        pub number: String,
        /// expense | income
        pub receipt_type: String,
        pub contact_id: Option<i64>,
        pub supplier_name: String,
        pub invoice_number: String,
        pub date: NaiveDate,
        pub due_date: Option<NaiveDate>,
        pub service_date: Option<NaiveDate>,
        pub description: String,
        /// standard | rc_eu | rc_other | eu_acquisition
        pub tax_mode: String,
        pub prices_gross: bool,
        #[ts(type = "number")]
        pub net_total: Decimal,
        #[ts(type = "number")]
        pub tax_total: Decimal,
        #[ts(type = "number")]
        pub gross_total: Decimal,
        #[ts(type = "number")]
        pub paid_total: Decimal,
        /// open | partial | paid
        pub status: String,
        pub file_id: Option<i64>,
        /// manual | xrechnung | zugferd
        pub source: String,
        pub iban: String,
        pub notes: String,
        pub created_at: String,
        pub updated_at: String,
    }
}

impl Receipt {
    /// +1 when money flows in (income receipt), -1 for expenses.
    pub fn sign(&self) -> Decimal {
        if self.receipt_type == "income" { Decimal::ONE } else { Decimal::NEGATIVE_ONE }
    }
    pub fn open_amount(&self) -> Decimal {
        self.gross_total - self.paid_total
    }
    /// Direction of the money that settles the receipt: +1 if it is received (income, supplier
    /// credit note), -1 if it is paid out.
    pub fn money_sign(&self) -> Decimal {
        self.sign() * self.gross_total.signum()
    }
    /// Amount still to be paid or refunded – positive while the receipt is open.
    pub fn remaining(&self) -> Decimal {
        self.open_amount() * self.gross_total.signum()
    }
}

entity! {
    pub struct ReceiptLine in "receipt_lines" {
        pub id: i64,
        pub receipt_id: i64,
        pub position: i64,
        pub account: String,
        pub description: String,
        #[ts(type = "number")]
        pub net: Decimal,
        #[ts(type = "number")]
        pub tax_rate: Decimal,
        #[ts(type = "number")]
        pub tax: Decimal,
        #[ts(type = "number")]
        pub gross: Decimal,
        pub asset_id: Option<i64>,
        pub asset_life_years: i64,
    }
}

// ---------------------------------------------------------------------------
// Anlagen, Buchungen
// ---------------------------------------------------------------------------

entity! {
    pub struct Asset in "assets" {
        pub id: i64,
        pub number: String,
        pub name: String,
        pub account: String,
        pub acquisition_date: NaiveDate,
        #[ts(type = "number")]
        pub cost: Decimal,
        pub useful_life_years: i64,
        /// linear | gwg | pool | none
        pub method: String,
        pub receipt_id: Option<i64>,
        pub receipt_line_id: Option<i64>,
        pub disposal_date: Option<NaiveDate>,
        #[ts(type = "number")]
        pub disposal_proceeds: Decimal,
        pub notes: String,
        pub created_at: String,
    }
}

entity! {
    /// Manueller Buchungssatz (Soll an Haben).
    pub struct ManualEntry in "manual_entries" {
        pub id: i64,
        pub date: NaiveDate,
        pub description: String,
        pub doc_ref: String,
        pub debit_account: String,
        pub credit_account: String,
        #[ts(type = "number")]
        pub amount: Decimal,
        /// "" | VST19 | VST7 | UST19 | UST7
        pub tax_key: String,
        pub created_at: String,
    }
}

entity! {
    pub struct JournalEntry in "journal_entries" {
        pub id: i64,
        pub date: NaiveDate,
        pub description: String,
        pub doc_ref: String,
        pub source_type: String,
        pub source_id: i64,
        pub contact_id: Option<i64>,
        pub created_at: String,
    }
}

entity! {
    pub struct JournalLine in "journal_lines" {
        pub id: i64,
        pub entry_id: i64,
        pub account: String,
        #[ts(type = "number")]
        pub debit: Decimal,
        #[ts(type = "number")]
        pub credit: Decimal,
        pub tax_code: String,
        /// Bemessungsgrundlage for tax lines (reverse charge, intra-EU acquisitions).
        #[ts(type = "number")]
        pub base: Decimal,
    }
}

entity! {
    pub struct Dunning in "dunnings" {
        pub id: i64,
        pub document_id: i64,
        pub level: i64,
        pub date: NaiveDate,
        pub due_date: NaiveDate,
        #[ts(type = "number")]
        pub fee: Decimal,
        #[ts(type = "number")]
        pub interest: Decimal,
        #[ts(type = "number")]
        pub open_amount: Decimal,
        pub title: String,
        pub text: String,
        pub created_at: String,
    }
}

entity! {
    pub struct Activity in "activities" {
        pub id: i64,
        pub entity: String,
        pub entity_id: i64,
        pub action: String,
        pub message: String,
        pub created_at: String,
    }
}

entity! {
    /// Abo-Rechnung: creates invoice drafts from a template invoice.
    pub struct RecurringInvoice in "recurring_invoices" {
        pub id: i64,
        pub template_id: i64,
        /// monthly | quarterly | halfyearly | yearly
        pub cycle: String,
        /// Day of the month the invoices are dated (short months use their last day).
        pub day: i64,
        pub next_date: NaiveDate,
        pub end_date: Option<NaiveDate>,
        pub active: bool,
        pub last_document_id: Option<i64>,
        pub created_at: String,
        /// Finalize the created invoices right away instead of leaving drafts.
        pub auto_finalize: bool,
        /// Send finalized invoices by e-mail to the customer (needs SMTP).
        pub auto_send: bool,
        /// Price change of the item prices, e.g. 3 = +3 %; 0 = none.
        #[ts(type = "number")]
        pub price_change_percent: Decimal,
        /// First period with the changed prices.
        pub price_change_from: Option<NaiveDate>,
        /// Apply the change again every year after `price_change_from`.
        pub price_change_yearly: bool,
        /// Why the last automatic finalizing or sending failed; empty after a successful run.
        pub last_error: String,
    }
}

entity! {
    /// SEPA-Lastschriftmandat granted by a customer.
    pub struct Mandate in "mandates" {
        pub id: i64,
        pub contact_id: i64,
        /// Mandatsreferenz (unique, at most 35 characters).
        pub reference: String,
        pub signed_on: NaiveDate,
        /// CORE (Basislastschrift) | B2B (Firmenlastschrift)
        pub scheme: String,
        /// Recurring collections; otherwise a one-off mandate for a single collection.
        pub recurring: bool,
        pub account_holder: String,
        pub iban: String,
        pub bic: String,
        pub revoked_on: Option<NaiveDate>,
        pub notes: String,
        pub created_at: String,
    }
}

entity! {
    /// SEPA direct debit file (pain.008) for open invoices; the file itself is in the `xml` column.
    pub struct DebitRun in "debit_runs" {
        pub id: i64,
        pub account_id: i64,
        pub message_id: String,
        #[ts(type = "number")]
        pub total: Decimal,
        pub count: i64,
        pub created_at: String,
    }
}

entity! {
    pub struct DebitRunItem in "debit_run_items" {
        pub id: i64,
        pub run_id: i64,
        pub document_id: i64,
        pub mandate_id: i64,
        #[ts(type = "number")]
        pub amount: Decimal,
        pub collection_date: NaiveDate,
        /// FRST | RCUR | OOFF
        pub sequence: String,
        /// CORE | B2B
        pub scheme: String,
        pub debtor_name: String,
        pub debtor_iban: String,
        pub debtor_bic: String,
        pub mandate_reference: String,
        pub mandate_signed_on: NaiveDate,
        pub remittance: String,
        pub end_to_end_id: String,
    }
}

entity! {
    /// Booking rule for bank movements (Bankregel).
    pub struct BankRule in "bank_rules" {
        pub id: i64,
        pub name: String,
        /// Part of the counterparty name (case-insensitive).
        pub counterparty: String,
        /// IBAN of the counterparty.
        pub iban: String,
        /// Part of the purpose (case-insensitive).
        pub purpose: String,
        /// "" | in | out
        pub direction: String,
        pub account: String,
        #[ts(type = "number")]
        pub tax_rate: Decimal,
        /// Booking text; empty = purpose of the movement.
        pub description: String,
        pub active: bool,
        pub created_at: String,
    }
}

entity! {
    pub struct UstvaFiling in "ustva_filings" {
        pub id: i64,
        pub year: i64,
        pub period: String,
        pub submitted_at: String,
        #[ts(type = "number")]
        pub amount: Decimal,
        pub note: String,
    }
}

entity! {
    /// SEPA credit transfer file (pain.001) created for open receipts; the file itself is in the
    /// `xml` column.
    pub struct PaymentRun in "payment_runs" {
        pub id: i64,
        pub account_id: i64,
        pub execution_date: NaiveDate,
        pub message_id: String,
        #[ts(type = "number")]
        pub total: Decimal,
        pub count: i64,
        pub created_at: String,
    }
}

entity! {
    pub struct PaymentRunItem in "payment_run_items" {
        pub id: i64,
        pub run_id: i64,
        pub receipt_id: i64,
        #[ts(type = "number")]
        pub amount: Decimal,
        pub creditor_name: String,
        pub creditor_iban: String,
        pub creditor_bic: String,
        pub remittance: String,
        pub end_to_end_id: String,
    }
}

entity! {
    /// A Zusammenfassende Meldung marked as submitted; `content` holds the reported CSV rows.
    pub struct ZmFiling in "zm_filings" {
        pub id: i64,
        pub year: i64,
        pub period: String,
        pub submitted_at: String,
        #[ts(type = "number")]
        pub amount: Decimal,
        pub content: String,
        pub note: String,
    }
}

// ---------------------------------------------------------------------------
// Einstellungen
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct CompanySettings {
    pub name: String,
    pub legal_form: String,
    pub owner: String,
    pub street: String,
    pub zip: String,
    pub city: String,
    pub country: String,
    pub phone: String,
    pub email: String,
    pub website: String,
    pub tax_number: String,
    pub vat_id: String,
    pub register_court: String,
    pub register_number: String,
    pub bank_name: String,
    pub iban: String,
    pub bic: String,
    pub logo_file_id: Option<i64>,
    pub contact_name: String,
}

impl Default for CompanySettings {
    fn default() -> Self {
        Self {
            name: String::new(),
            legal_form: String::new(),
            owner: String::new(),
            street: String::new(),
            zip: String::new(),
            city: String::new(),
            country: "DE".into(),
            phone: String::new(),
            email: String::new(),
            website: String::new(),
            tax_number: String::new(),
            vat_id: String::new(),
            register_court: String::new(),
            register_number: String::new(),
            bank_name: String::new(),
            iban: String::new(),
            bic: String::new(),
            logo_file_id: None,
            contact_name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct TaxSettings {
    /// Kleinunternehmer nach § 19 UStG
    pub small_business: bool,
    /// euer | bilanz
    pub bookkeeping: String,
    /// SKR03 | SKR04
    pub chart: String,
    /// monthly | quarterly | yearly
    pub ustva_period: String,
    pub permanent_extension: bool,
    /// Zusammenfassende Meldung: monthly | quarterly
    pub zm_period: String,
    pub locked_until: Option<NaiveDate>,
    #[ts(type = "number")]
    pub default_tax_rate: Decimal,
}

impl Default for TaxSettings {
    fn default() -> Self {
        Self {
            small_business: false,
            bookkeeping: "euer".into(),
            chart: "SKR03".into(),
            ustva_period: "quarterly".into(),
            permanent_extension: false,
            zm_period: "quarterly".into(),
            locked_until: None,
            default_tax_rate: Decimal::from(19),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct DocTexts {
    pub title: String,
    pub intro: String,
    pub closing: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct DocSettings {
    pub payment_terms_days: i64,
    pub quote_validity_days: i64,
    pub default_layout_id: Option<i64>,
    pub default_unit: String,
    pub einvoice_default: String,
    /// Keys: quote, order, delivery, invoice, credit_note, partial, final, cancellation
    pub texts: BTreeMap<String, DocTexts>,
}

impl Default for DocSettings {
    fn default() -> Self {
        Self {
            payment_terms_days: 14,
            quote_validity_days: 30,
            default_layout_id: None,
            default_unit: "Stk".into(),
            einvoice_default: "pdf".into(),
            texts: crate::services::defaults::default_texts(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct DunningLevel {
    pub level: i64,
    pub name: String,
    /// Days after due date (level 0) or after the previous deadline.
    pub days: i64,
    /// New payment deadline in days from the dunning date.
    pub deadline_days: i64,
    #[ts(type = "number")]
    pub fee: Decimal,
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct DunningSettings {
    pub levels: Vec<DunningLevel>,
    pub interest_enabled: bool,
    #[ts(type = "number")]
    pub base_rate: Decimal,
    /// Since when the entered Basiszinssatz applies (it changes on 1 January and 1 July).
    pub base_rate_since: Option<NaiveDate>,
    #[ts(type = "number")]
    pub surcharge_b2b: Decimal,
    #[ts(type = "number")]
    pub surcharge_b2c: Decimal,
    /// Charge the Verzugspauschale of 40 € to business customers (§ 288 Abs. 5 BGB).
    pub flat_fee_b2b: bool,
}

impl Default for DunningSettings {
    fn default() -> Self {
        Self {
            levels: crate::services::defaults::default_dunning_levels(),
            interest_enabled: false,
            base_rate: Decimal::new(127, 2),
            base_rate_since: None,
            surcharge_b2b: Decimal::from(9),
            surcharge_b2c: Decimal::from(5),
            flat_fee_b2b: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Settings {
    pub onboarded: bool,
    pub company: CompanySettings,
    pub tax: TaxSettings,
    pub docs: DocSettings,
    pub dunning: DunningSettings,
    /// Hides the "Erste Schritte" checklist on the dashboard.
    pub hide_getting_started: bool,
    pub backup: BackupSettings,
    pub email: EmailSettings,
    pub updates: UpdateSettings,
    pub debit: DebitSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct DebitSettings {
    /// Gläubiger-Identifikationsnummer (from the Deutsche Bundesbank).
    pub creditor_id: String,
    /// Days between the pre-notification (the invoice) and the collection; 14 unless a shorter
    /// period was agreed with the customers.
    pub prenotification_days: i64,
}

impl Default for DebitSettings {
    fn default() -> Self {
        Self { creditor_id: String::new(), prenotification_days: 14 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct UpdateSettings {
    /// Look for a new version on GitHub at the start and daily while the program runs.
    pub auto_check: bool,
    /// Version the user does not want to be reminded of.
    pub skipped_version: String,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto_check: true, skipped_version: String::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct EmailSettings {
    /// Send documents from Sorobo (otherwise the mail program is opened).
    pub enabled: bool,
    pub host: String,
    pub port: i64,
    /// starttls | tls | none
    pub security: String,
    pub username: String,
    /// Never sent to the frontend; saving an empty value keeps the stored password.
    pub password: String,
    /// Set when reading the settings: a password is stored.
    pub has_password: bool,
    pub from_name: String,
    pub from_address: String,
    /// Send a blind copy to the sender address.
    pub bcc_self: bool,
    /// Templates with the placeholders of the document texts plus {{Dokument}}.
    pub subject: String,
    pub body: String,
}

impl Default for EmailSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: 587,
            security: "starttls".into(),
            username: String::new(),
            password: String::new(),
            has_password: false,
            from_name: String::new(),
            from_address: String::new(),
            bcc_self: true,
            subject: "{{Dokument}} {{Nummer}}".into(),
            body: "{{Anrede}}\n\nanbei erhalten Sie unsere {{Dokument}} {{Nummer}} vom {{Datum}} über {{Betrag}}.\n\nMit freundlichen Grüßen\n{{Firma}}".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct BackupSettings {
    /// Daily automatic backup.
    pub enabled: bool,
    /// Target folder; empty = `backups` in the data folder.
    pub folder: String,
    /// Number of automatic backups that are kept.
    pub keep: i64,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self { enabled: true, folder: String::new(), keep: 30 }
    }
}
