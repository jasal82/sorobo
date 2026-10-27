-- Copyright (C) 2026 Johannes Asal
-- SPDX-License-Identifier: AGPL-3.0-or-later
-- See LICENSE in the project root for the full license and warranty disclaimer.

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
  id         INTEGER PRIMARY KEY,
  uuid       TEXT NOT NULL UNIQUE,
  filename   TEXT NOT NULL,
  mime       TEXT NOT NULL,
  size       INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS accounts (
  id         INTEGER PRIMARY KEY,
  number     TEXT NOT NULL UNIQUE,
  name       TEXT NOT NULL,
  kind       TEXT NOT NULL,
  euer       TEXT NOT NULL DEFAULT '',
  guv        TEXT NOT NULL DEFAULT '',
  bwa        TEXT NOT NULL DEFAULT '',
  bilanz     TEXT NOT NULL DEFAULT '',
  tax_rate   TEXT,
  tax_code   TEXT NOT NULL DEFAULT '',
  role       TEXT NOT NULL DEFAULT '',
  datev_auto INTEGER NOT NULL DEFAULT 0,
  pick       TEXT NOT NULL DEFAULT '',
  hint       TEXT NOT NULL DEFAULT '',
  system     INTEGER NOT NULL DEFAULT 0,
  active     INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS number_ranges (
  id           INTEGER PRIMARY KEY,
  key          TEXT NOT NULL UNIQUE,
  label        TEXT NOT NULL DEFAULT '',
  format       TEXT NOT NULL,
  next         INTEGER NOT NULL DEFAULT 1,
  year         INTEGER NOT NULL DEFAULT 0,
  reset_yearly INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS contacts (
  id                 INTEGER PRIMARY KEY,
  number             TEXT NOT NULL DEFAULT '',
  is_customer        INTEGER NOT NULL DEFAULT 1,
  is_supplier        INTEGER NOT NULL DEFAULT 0,
  kind               TEXT NOT NULL DEFAULT 'organization',
  company            TEXT NOT NULL DEFAULT '',
  salutation         TEXT NOT NULL DEFAULT '',
  title              TEXT NOT NULL DEFAULT '',
  first_name         TEXT NOT NULL DEFAULT '',
  last_name          TEXT NOT NULL DEFAULT '',
  address_extra      TEXT NOT NULL DEFAULT '',
  street             TEXT NOT NULL DEFAULT '',
  zip                TEXT NOT NULL DEFAULT '',
  city               TEXT NOT NULL DEFAULT '',
  country            TEXT NOT NULL DEFAULT 'DE',
  email              TEXT NOT NULL DEFAULT '',
  phone              TEXT NOT NULL DEFAULT '',
  mobile             TEXT NOT NULL DEFAULT '',
  website            TEXT NOT NULL DEFAULT '',
  vat_id             TEXT NOT NULL DEFAULT '',
  tax_number         TEXT NOT NULL DEFAULT '',
  leitweg_id         TEXT NOT NULL DEFAULT '',
  iban               TEXT NOT NULL DEFAULT '',
  bic                TEXT NOT NULL DEFAULT '',
  payment_terms_days INTEGER,
  discount_percent   TEXT,
  einvoice_format    TEXT NOT NULL DEFAULT 'pdf',
  default_tax_mode   TEXT NOT NULL DEFAULT '',
  persons            TEXT NOT NULL DEFAULT '[]',
  notes              TEXT NOT NULL DEFAULT '',
  tags               TEXT NOT NULL DEFAULT '[]',
  archived           INTEGER NOT NULL DEFAULT 0,
  created_at         TEXT NOT NULL DEFAULT '',
  updated_at         TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS articles (
  id              INTEGER PRIMARY KEY,
  number          TEXT NOT NULL DEFAULT '',
  kind            TEXT NOT NULL DEFAULT 'product',
  name            TEXT NOT NULL,
  description     TEXT NOT NULL DEFAULT '',
  unit            TEXT NOT NULL DEFAULT 'Stk',
  price           TEXT NOT NULL DEFAULT '0',
  purchase_price  TEXT NOT NULL DEFAULT '0',
  tax_rate        TEXT NOT NULL DEFAULT '19',
  revenue_account TEXT NOT NULL DEFAULT '',
  category        TEXT NOT NULL DEFAULT '',
  gtin            TEXT NOT NULL DEFAULT '',
  track_stock     INTEGER NOT NULL DEFAULT 0,
  stock           TEXT NOT NULL DEFAULT '0',
  archived        INTEGER NOT NULL DEFAULT 0,
  created_at      TEXT NOT NULL DEFAULT '',
  updated_at      TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS layouts (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  is_default INTEGER NOT NULL DEFAULT 0,
  config     TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS documents (
  id                    INTEGER PRIMARY KEY,
  doc_type              TEXT NOT NULL,
  kind                  TEXT NOT NULL DEFAULT 'standard',
  status                TEXT NOT NULL DEFAULT 'draft',
  number                TEXT NOT NULL DEFAULT '',
  contact_id            INTEGER REFERENCES contacts(id),
  address               TEXT NOT NULL DEFAULT '',
  contact_person        TEXT NOT NULL DEFAULT '',
  date                  TEXT NOT NULL,
  service_date          TEXT,
  service_date_end      TEXT,
  due_date              TEXT,
  payment_terms_days    INTEGER,
  valid_until           TEXT,
  title                 TEXT NOT NULL DEFAULT '',
  reference             TEXT NOT NULL DEFAULT '',
  buyer_reference       TEXT NOT NULL DEFAULT '',
  intro_text            TEXT NOT NULL DEFAULT '',
  closing_text          TEXT NOT NULL DEFAULT '',
  tax_mode              TEXT NOT NULL DEFAULT 'standard',
  prices_gross          INTEGER NOT NULL DEFAULT 0,
  cash_discount_percent TEXT NOT NULL DEFAULT '0',
  cash_discount_days    INTEGER NOT NULL DEFAULT 0,
  layout_id             INTEGER,
  currency              TEXT NOT NULL DEFAULT 'EUR',
  net_total             TEXT NOT NULL DEFAULT '0',
  tax_total             TEXT NOT NULL DEFAULT '0',
  gross_total           TEXT NOT NULL DEFAULT '0',
  paid_total            TEXT NOT NULL DEFAULT '0',
  source_id             INTEGER,
  base_id               INTEGER,
  partial_mode          TEXT NOT NULL DEFAULT '',
  partial_value         TEXT NOT NULL DEFAULT '0',
  cancels_id            INTEGER,
  einvoice_format       TEXT NOT NULL DEFAULT '',
  dunning_level         INTEGER NOT NULL DEFAULT 0,
  finalized_at          TEXT NOT NULL DEFAULT '',
  sent_at               TEXT NOT NULL DEFAULT '',
  paid_at               TEXT,
  notes                 TEXT NOT NULL DEFAULT '',
  created_at            TEXT NOT NULL DEFAULT '',
  updated_at            TEXT NOT NULL DEFAULT '',
  payment_method        TEXT NOT NULL DEFAULT '',
  mandate_id            INTEGER
);
CREATE INDEX IF NOT EXISTS idx_documents_type ON documents(doc_type, status);
CREATE INDEX IF NOT EXISTS idx_documents_contact ON documents(contact_id);

CREATE TABLE IF NOT EXISTS document_lines (
  id               INTEGER PRIMARY KEY,
  document_id      INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  position         INTEGER NOT NULL DEFAULT 0,
  line_type        TEXT NOT NULL DEFAULT 'item',
  article_id       INTEGER,
  article_number   TEXT NOT NULL DEFAULT '',
  name             TEXT NOT NULL DEFAULT '',
  description      TEXT NOT NULL DEFAULT '',
  quantity         TEXT NOT NULL DEFAULT '1',
  unit             TEXT NOT NULL DEFAULT '',
  unit_price       TEXT NOT NULL DEFAULT '0',
  discount_percent TEXT NOT NULL DEFAULT '0',
  tax_rate         TEXT NOT NULL DEFAULT '0',
  account          TEXT NOT NULL DEFAULT '',
  net_amount       TEXT NOT NULL DEFAULT '0',
  tax_amount       TEXT NOT NULL DEFAULT '0',
  gross_amount     TEXT NOT NULL DEFAULT '0',
  ref_document_id  INTEGER
);
CREATE INDEX IF NOT EXISTS idx_document_lines_doc ON document_lines(document_id);

CREATE TABLE IF NOT EXISTS payment_accounts (
  id              INTEGER PRIMARY KEY,
  name            TEXT NOT NULL,
  kind            TEXT NOT NULL DEFAULT 'bank',
  iban            TEXT NOT NULL DEFAULT '',
  bic             TEXT NOT NULL DEFAULT '',
  bank_name       TEXT NOT NULL DEFAULT '',
  ledger_account  TEXT NOT NULL,
  opening_balance TEXT NOT NULL DEFAULT '0',
  opening_date    TEXT NOT NULL,
  color           TEXT NOT NULL DEFAULT '',
  archived        INTEGER NOT NULL DEFAULT 0,
  created_at      TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS transactions (
  id                INTEGER PRIMARY KEY,
  account_id        INTEGER NOT NULL REFERENCES payment_accounts(id),
  date              TEXT NOT NULL,
  amount            TEXT NOT NULL,
  counterparty      TEXT NOT NULL DEFAULT '',
  counterparty_iban TEXT NOT NULL DEFAULT '',
  purpose           TEXT NOT NULL DEFAULT '',
  cash_number       INTEGER,
  status            TEXT NOT NULL DEFAULT 'open',
  import_hash       TEXT NOT NULL DEFAULT '',
  source            TEXT NOT NULL DEFAULT 'manual',
  reversal_of       INTEGER,
  created_at        TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_transactions_account ON transactions(account_id, date);

CREATE TABLE IF NOT EXISTS allocations (
  id             INTEGER PRIMARY KEY,
  transaction_id INTEGER REFERENCES transactions(id) ON DELETE CASCADE,
  kind           TEXT NOT NULL,
  document_id    INTEGER REFERENCES documents(id),
  receipt_id     INTEGER REFERENCES receipts(id),
  account        TEXT NOT NULL DEFAULT '',
  tax_rate       TEXT NOT NULL DEFAULT '0',
  amount         TEXT NOT NULL,
  date           TEXT NOT NULL,
  writeoff_reason TEXT NOT NULL DEFAULT '',
  description    TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_allocations_tx ON allocations(transaction_id);
CREATE INDEX IF NOT EXISTS idx_allocations_doc ON allocations(document_id);
CREATE INDEX IF NOT EXISTS idx_allocations_receipt ON allocations(receipt_id);

-- Abo-Rechnungen: invoice drafts created from a template invoice
CREATE TABLE IF NOT EXISTS recurring_invoices (
  id               INTEGER PRIMARY KEY,
  template_id      INTEGER NOT NULL REFERENCES documents(id),
  cycle            TEXT NOT NULL DEFAULT 'monthly',
  day              INTEGER NOT NULL DEFAULT 1,
  next_date        TEXT NOT NULL,
  end_date         TEXT,
  active           INTEGER NOT NULL DEFAULT 1,
  last_document_id INTEGER,
  created_at       TEXT NOT NULL DEFAULT '',
  auto_finalize    INTEGER NOT NULL DEFAULT 0,
  auto_send        INTEGER NOT NULL DEFAULT 0,
  price_change_percent TEXT NOT NULL DEFAULT '0',
  price_change_from    TEXT,
  price_change_yearly  INTEGER NOT NULL DEFAULT 0,
  last_error       TEXT NOT NULL DEFAULT ''
);

-- Bankregeln: automatic booking of recurring bank movements
CREATE TABLE IF NOT EXISTS bank_rules (
  id           INTEGER PRIMARY KEY,
  name         TEXT NOT NULL DEFAULT '',
  counterparty TEXT NOT NULL DEFAULT '',
  iban         TEXT NOT NULL DEFAULT '',
  purpose      TEXT NOT NULL DEFAULT '',
  direction    TEXT NOT NULL DEFAULT '',
  account      TEXT NOT NULL,
  tax_rate     TEXT NOT NULL DEFAULT '0',
  description  TEXT NOT NULL DEFAULT '',
  active       INTEGER NOT NULL DEFAULT 1,
  created_at   TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS receipts (
  id             INTEGER PRIMARY KEY,
  number         TEXT NOT NULL DEFAULT '',
  receipt_type   TEXT NOT NULL DEFAULT 'expense',
  contact_id     INTEGER REFERENCES contacts(id),
  supplier_name  TEXT NOT NULL DEFAULT '',
  invoice_number TEXT NOT NULL DEFAULT '',
  date           TEXT NOT NULL,
  due_date       TEXT,
  service_date   TEXT,
  description    TEXT NOT NULL DEFAULT '',
  tax_mode       TEXT NOT NULL DEFAULT 'standard',
  prices_gross   INTEGER NOT NULL DEFAULT 1,
  net_total      TEXT NOT NULL DEFAULT '0',
  tax_total      TEXT NOT NULL DEFAULT '0',
  gross_total    TEXT NOT NULL DEFAULT '0',
  paid_total     TEXT NOT NULL DEFAULT '0',
  status         TEXT NOT NULL DEFAULT 'open',
  file_id        INTEGER REFERENCES files(id),
  source         TEXT NOT NULL DEFAULT 'manual',
  iban           TEXT NOT NULL DEFAULT '',
  notes          TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL DEFAULT '',
  updated_at     TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS receipt_lines (
  id               INTEGER PRIMARY KEY,
  receipt_id       INTEGER NOT NULL REFERENCES receipts(id) ON DELETE CASCADE,
  position         INTEGER NOT NULL DEFAULT 0,
  account          TEXT NOT NULL,
  description      TEXT NOT NULL DEFAULT '',
  net              TEXT NOT NULL DEFAULT '0',
  tax_rate         TEXT NOT NULL DEFAULT '0',
  tax              TEXT NOT NULL DEFAULT '0',
  gross            TEXT NOT NULL DEFAULT '0',
  asset_id         INTEGER,
  asset_life_years INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_receipt_lines_receipt ON receipt_lines(receipt_id);

CREATE TABLE IF NOT EXISTS assets (
  id                INTEGER PRIMARY KEY,
  number            TEXT NOT NULL DEFAULT '',
  name              TEXT NOT NULL,
  account           TEXT NOT NULL,
  acquisition_date  TEXT NOT NULL,
  cost              TEXT NOT NULL DEFAULT '0',
  useful_life_years INTEGER NOT NULL DEFAULT 1,
  method            TEXT NOT NULL DEFAULT 'linear',
  receipt_id        INTEGER,
  receipt_line_id   INTEGER,
  disposal_date     TEXT,
  disposal_proceeds TEXT NOT NULL DEFAULT '0',
  notes             TEXT NOT NULL DEFAULT '',
  created_at        TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS manual_entries (
  id             INTEGER PRIMARY KEY,
  date           TEXT NOT NULL,
  description    TEXT NOT NULL DEFAULT '',
  doc_ref        TEXT NOT NULL DEFAULT '',
  debit_account  TEXT NOT NULL,
  credit_account TEXT NOT NULL,
  amount         TEXT NOT NULL,
  tax_key        TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS journal_entries (
  id          INTEGER PRIMARY KEY,
  date        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  doc_ref     TEXT NOT NULL DEFAULT '',
  source_type TEXT NOT NULL,
  source_id   INTEGER NOT NULL,
  contact_id  INTEGER,
  created_at  TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_journal_source ON journal_entries(source_type, source_id);
CREATE INDEX IF NOT EXISTS idx_journal_date ON journal_entries(date);

CREATE TABLE IF NOT EXISTS journal_lines (
  id       INTEGER PRIMARY KEY,
  entry_id INTEGER NOT NULL REFERENCES journal_entries(id) ON DELETE CASCADE,
  account  TEXT NOT NULL,
  debit    TEXT NOT NULL DEFAULT '0',
  credit   TEXT NOT NULL DEFAULT '0',
  tax_code TEXT NOT NULL DEFAULT '',
  base     TEXT NOT NULL DEFAULT '0'
);
CREATE INDEX IF NOT EXISTS idx_journal_lines_entry ON journal_lines(entry_id);
CREATE INDEX IF NOT EXISTS idx_journal_lines_account ON journal_lines(account);

-- Files of issued invoices / credit notes as issued (GoBD): pdf | pages | zugferd | xrechnung
CREATE TABLE IF NOT EXISTS document_archive (
  id          INTEGER PRIMARY KEY,
  document_id INTEGER NOT NULL REFERENCES documents(id),
  kind        TEXT NOT NULL,
  file_id     INTEGER NOT NULL REFERENCES files(id),
  created_at  TEXT NOT NULL DEFAULT '',
  UNIQUE(document_id, kind)
);

CREATE TABLE IF NOT EXISTS dunnings (
  id          INTEGER PRIMARY KEY,
  document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  level       INTEGER NOT NULL DEFAULT 0,
  date        TEXT NOT NULL,
  due_date    TEXT NOT NULL,
  fee         TEXT NOT NULL DEFAULT '0',
  interest    TEXT NOT NULL DEFAULT '0',
  open_amount TEXT NOT NULL DEFAULT '0',
  title       TEXT NOT NULL DEFAULT '',
  text        TEXT NOT NULL DEFAULT '',
  created_at  TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS activities (
  id         INTEGER PRIMARY KEY,
  entity     TEXT NOT NULL,
  entity_id  INTEGER NOT NULL,
  action     TEXT NOT NULL,
  message    TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_activities_entity ON activities(entity, entity_id);

CREATE TABLE IF NOT EXISTS ustva_filings (
  id           INTEGER PRIMARY KEY,
  year         INTEGER NOT NULL,
  period       TEXT NOT NULL,
  submitted_at TEXT NOT NULL DEFAULT '',
  amount       TEXT NOT NULL DEFAULT '0',
  note         TEXT NOT NULL DEFAULT '',
  UNIQUE(year, period)
);

CREATE TABLE IF NOT EXISTS payment_runs (
  id             INTEGER PRIMARY KEY,
  account_id     INTEGER NOT NULL REFERENCES payment_accounts(id),
  execution_date TEXT NOT NULL,
  message_id     TEXT NOT NULL DEFAULT '',
  total          TEXT NOT NULL DEFAULT '0',
  count          INTEGER NOT NULL DEFAULT 0,
  xml            TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS payment_run_items (
  id            INTEGER PRIMARY KEY,
  run_id        INTEGER NOT NULL REFERENCES payment_runs(id) ON DELETE CASCADE,
  receipt_id    INTEGER NOT NULL REFERENCES receipts(id),
  amount        TEXT NOT NULL DEFAULT '0',
  creditor_name TEXT NOT NULL DEFAULT '',
  creditor_iban TEXT NOT NULL DEFAULT '',
  creditor_bic  TEXT NOT NULL DEFAULT '',
  remittance    TEXT NOT NULL DEFAULT '',
  end_to_end_id TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_payment_run_items_receipt ON payment_run_items(receipt_id);

-- SEPA-Lastschriften: mandates of customers and pain.008 files for open invoices
CREATE TABLE IF NOT EXISTS mandates (
  id             INTEGER PRIMARY KEY,
  contact_id     INTEGER NOT NULL REFERENCES contacts(id),
  reference      TEXT NOT NULL UNIQUE,
  signed_on      TEXT NOT NULL,
  scheme         TEXT NOT NULL DEFAULT 'CORE',
  recurring      INTEGER NOT NULL DEFAULT 1,
  account_holder TEXT NOT NULL DEFAULT '',
  iban           TEXT NOT NULL DEFAULT '',
  bic            TEXT NOT NULL DEFAULT '',
  revoked_on     TEXT,
  notes          TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_mandates_contact ON mandates(contact_id);

CREATE TABLE IF NOT EXISTS debit_runs (
  id         INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES payment_accounts(id),
  message_id TEXT NOT NULL DEFAULT '',
  total      TEXT NOT NULL DEFAULT '0',
  count      INTEGER NOT NULL DEFAULT 0,
  xml        TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS debit_run_items (
  id                INTEGER PRIMARY KEY,
  run_id            INTEGER NOT NULL REFERENCES debit_runs(id) ON DELETE CASCADE,
  document_id       INTEGER NOT NULL REFERENCES documents(id),
  mandate_id        INTEGER NOT NULL REFERENCES mandates(id),
  amount            TEXT NOT NULL DEFAULT '0',
  collection_date   TEXT NOT NULL,
  sequence          TEXT NOT NULL DEFAULT 'RCUR',
  scheme            TEXT NOT NULL DEFAULT 'CORE',
  debtor_name       TEXT NOT NULL DEFAULT '',
  debtor_iban       TEXT NOT NULL DEFAULT '',
  debtor_bic        TEXT NOT NULL DEFAULT '',
  mandate_reference TEXT NOT NULL DEFAULT '',
  mandate_signed_on TEXT NOT NULL,
  remittance        TEXT NOT NULL DEFAULT '',
  end_to_end_id     TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_debit_run_items_document ON debit_run_items(document_id);

CREATE TABLE IF NOT EXISTS zm_filings (
  id           INTEGER PRIMARY KEY,
  year         INTEGER NOT NULL,
  period       TEXT NOT NULL,
  submitted_at TEXT NOT NULL DEFAULT '',
  amount       TEXT NOT NULL DEFAULT '0',
  content      TEXT NOT NULL DEFAULT '',
  note         TEXT NOT NULL DEFAULT '',
  UNIQUE(year, period)
);

CREATE TABLE IF NOT EXISTS people (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  archived   INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS time_entries (
  id          INTEGER PRIMARY KEY,
  person_id   INTEGER NOT NULL REFERENCES people(id),
  document_id INTEGER NOT NULL REFERENCES documents(id),
  date        TEXT NOT NULL,
  hours       TEXT NOT NULL DEFAULT '0',
  offer_phase INTEGER NOT NULL DEFAULT 0,
  note        TEXT NOT NULL DEFAULT '',
  created_at  TEXT NOT NULL DEFAULT '',
  updated_at  TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_time_entries_document ON time_entries(document_id);
CREATE INDEX IF NOT EXISTS idx_time_entries_person ON time_entries(person_id, date);
