// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Database layer: SQLite connection, schema migration and a small typed
//! entity mapping (`entity!` macro) so that every table row maps 1:1 onto a
//! strongly typed Rust struct that is also exported to TypeScript.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, Row, params_from_iter};
use rust_decimal::Decimal;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// The current schema; fresh databases are created from it directly.
const SCHEMA: &str = include_str!("schema.sql");

/// Data changes since schema version 1: entry `n` (1-based) brings a database from version `n` to
/// `n + 1`. Tables and columns are not migrated here – `complete_tables` adds everything of
/// `schema.sql` an older database lacks, whatever state it is in. An entry is only needed to change
/// existing data (fill a new column, convert values); it runs after the tables are complete.
/// New columns still count the version up, so that an older Sorobo refuses the database instead
/// of writing rows without them.
const MIGRATIONS: &[&str] = &[
    // 1 → 2: Abo-Rechnungen finalize and send automatically, price changes (columns only)
    "",
    // 2 → 3: SEPA-Lastschrift as payment method of documents (columns only)
    "",
    // 3 → 4: Projektstunden (people, time_entries; tables only)
    "",
];

pub const SCHEMA_VERSION: i64 = 1 + MIGRATIONS.len() as i64;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("Datenbank {path:?} konnte nicht geöffnet werden"))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    if let Some(from) = stored_version(&conn)?.filter(|v| *v < SCHEMA_VERSION) {
        backup_before_upgrade(&conn, path, from)?;
    }
    migrate(&conn)?;
    Ok(conn)
}

/// Copy of the database before it is changed for a new version, in the `backups` folder next to
/// it (the automatic backups never delete it).
fn backup_before_upgrade(conn: &Connection, path: &Path, from: i64) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new(".")).join("backups");
    let target = dir.join(format!("Sorobo-vor-Update-Schema{from}-{SCHEMA_VERSION}_{}.db", chrono::Local::now().format("%Y-%m-%d_%H%M%S")));
    std::fs::create_dir_all(&dir)
        .map_err(anyhow::Error::from)
        .and_then(|_| conn.backup(rusqlite::MAIN_DB, &target, None::<fn(rusqlite::backup::Progress)>).map_err(anyhow::Error::from))
        .with_context(|| format!("Vor dem Datenbank-Update konnte keine Sicherung angelegt werden ({}). Die Datenbank wurde nicht verändert", target.display()))?;
    tracing::info!("Sicherung vor dem Datenbank-Update: {}", target.display());
    Ok(())
}

/// Fresh in-memory database with schema, for tests.
#[cfg(test)]
pub fn open_memory() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database");
    conn.pragma_update(None, "foreign_keys", "ON").expect("pragma");
    migrate(&conn).expect("schema");
    conn
}

fn migrate(conn: &Connection) -> Result<()> {
    migrate_with(conn, SCHEMA, MIGRATIONS)
}

/// Schema version stored in a database; None for a new, empty database.
pub fn schema_version(conn: &Connection) -> Option<i64> {
    let v: String = conn.query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0)).ok()?;
    v.parse().ok()
}

/// Version of an existing database: stored, or 1 for databases from before the version was
/// stored. None for a new, empty database.
fn stored_version(conn: &Connection) -> Result<Option<i64>> {
    if let Some(v) = schema_version(conn) {
        return Ok(Some(v));
    }
    let tables = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'", [], |r| r.get::<_, i64>(0))?;
    Ok((tables > 0).then_some(1))
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |r| r.get::<_, i64>(0))? > 0)
}

struct ColumnInfo {
    name: String,
    ty: String,
    not_null: bool,
    default: Option<String>,
}

fn columns(conn: &Connection, table: &str) -> Result<Vec<ColumnInfo>> {
    let mut stmt = conn.prepare("SELECT name, type, \"notnull\", dflt_value FROM pragma_table_info(?1) ORDER BY cid")?;
    let rows = stmt.query_map([table], |r| Ok(ColumnInfo { name: r.get(0)?, ty: r.get(1)?, not_null: r.get::<_, i64>(2)? != 0, default: r.get(3)? }))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Adds every table and column of `schema` that the database lacks: tables with their exact
/// definition, columns with type, NOT NULL and default (a NOT NULL column without default gets
/// the empty value of its type, as existing rows need one). Returns what was added.
fn complete_tables(conn: &Connection, schema: &str) -> Result<Vec<String>> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(schema)?;
    let mut stmt = reference.prepare("SELECT name, sql FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY rowid")?;
    let tables = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut added = Vec::new();
    for (table, sql) in tables {
        if !table_exists(conn, &table)? {
            conn.execute_batch(&sql).with_context(|| format!("Tabelle {table} konnte nicht angelegt werden"))?;
            added.push(table);
            continue;
        }
        let have: std::collections::HashSet<String> = columns(conn, &table)?.into_iter().map(|c| c.name.to_lowercase()).collect();
        for c in columns(&reference, &table)?.into_iter().filter(|c| !have.contains(&c.name.to_lowercase())) {
            let mut def = format!("\"{}\" {}", c.name, c.ty);
            if c.not_null {
                def.push_str(" NOT NULL");
            }
            match (&c.default, c.not_null) {
                (Some(d), _) => def.push_str(&format!(" DEFAULT {d}")),
                (None, true) => def.push_str(if ["INT", "REAL", "NUM"].iter().any(|t| c.ty.to_uppercase().contains(t)) { " DEFAULT 0" } else { " DEFAULT ''" }),
                (None, false) => {}
            }
            conn.execute_batch(&format!("ALTER TABLE \"{table}\" ADD COLUMN {def}")).with_context(|| format!("Spalte {table}.{} konnte nicht ergänzt werden", c.name))?;
            added.push(format!("{table}.{}", c.name));
        }
    }
    Ok(added)
}

/// Brings any older database to the current schema in one transaction – if anything fails,
/// nothing is changed.
fn migrate_with(conn: &Connection, schema: &str, migrations: &[&str]) -> Result<()> {
    let latest = 1 + migrations.len() as i64;
    let tx = conn.unchecked_transaction()?;
    let Some(version) = stored_version(&tx)? else {
        tx.execute_batch(schema)?;
        set_schema_version(&tx, latest)?;
        tx.commit()?;
        return Ok(());
    };
    if version > latest {
        anyhow::bail!(
            "Die Datenbank stammt aus einer neueren Sorobo-Version (Schema {version}, unterstützt bis {latest}). Bitte Sorobo aktualisieren."
        );
    }
    let added = complete_tables(&tx, schema).context("Datenbank-Update fehlgeschlagen")?;
    for (i, sql) in migrations.iter().enumerate().skip(version.max(1) as usize - 1) {
        let to = i as i64 + 2;
        tx.execute_batch(sql).with_context(|| format!("Datenbank-Migration auf Schema {to} fehlgeschlagen"))?;
    }
    // Indexes and anything else of the schema.
    tx.execute_batch(schema).context("Datenbank-Update fehlgeschlagen")?;
    set_schema_version(&tx, latest)?;
    tx.commit()?;
    if version < latest || !added.is_empty() {
        tracing::info!("Datenbank von Schema {version} auf {latest} aktualisiert; ergänzt: {}", if added.is_empty() { "–".to_string() } else { added.join(", ") });
    }
    Ok(())
}

fn set_schema_version(conn: &Connection, version: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [version.to_string()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Field conversion
// ---------------------------------------------------------------------------

/// Conversion between a struct field and a SQLite value.
pub trait Field: Sized {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self>;
    fn to_value(&self) -> Value;

    fn get(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        let v = row.get_ref(col)?;
        Self::from_ref(v).map_err(|e| match e {
            rusqlite::Error::FromSqlConversionFailure(i, t, err) => rusqlite::Error::FromSqlConversionFailure(
                i,
                t,
                format!("Spalte {col}: {err}").into(),
            ),
            other => other,
        })
    }
}

fn conv_err(msg: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, msg.into().into())
}

fn as_text(v: ValueRef<'_>) -> rusqlite::Result<String> {
    match v {
        ValueRef::Text(t) => Ok(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Integer(i) => Ok(i.to_string()),
        ValueRef::Real(f) => Ok(f.to_string()),
        ValueRef::Null => Ok(String::new()),
        ValueRef::Blob(b) => Ok(String::from_utf8_lossy(b).into_owned()),
    }
}

impl Field for i64 {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        match v {
            ValueRef::Integer(i) => Ok(i),
            ValueRef::Real(f) => Ok(f as i64),
            ValueRef::Null => Ok(0),
            ValueRef::Text(t) => String::from_utf8_lossy(t).trim().parse().map_err(|_| conv_err("keine Ganzzahl")),
            ValueRef::Blob(_) => Err(conv_err("Blob statt Ganzzahl")),
        }
    }
    fn to_value(&self) -> Value {
        Value::Integer(*self)
    }
}

impl Field for bool {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        Ok(i64::from_ref(v)? != 0)
    }
    fn to_value(&self) -> Value {
        Value::Integer(i64::from(*self))
    }
}

impl Field for f64 {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        match v {
            ValueRef::Integer(i) => Ok(i as f64),
            ValueRef::Real(f) => Ok(f),
            ValueRef::Null => Ok(0.0),
            ValueRef::Text(t) => String::from_utf8_lossy(t).trim().parse().map_err(|_| conv_err("keine Zahl")),
            ValueRef::Blob(_) => Err(conv_err("Blob statt Zahl")),
        }
    }
    fn to_value(&self) -> Value {
        Value::Real(*self)
    }
}

impl Field for String {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        as_text(v)
    }
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
}

impl Field for Decimal {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        match v {
            ValueRef::Null => Ok(Decimal::ZERO),
            ValueRef::Integer(i) => Ok(Decimal::from(i)),
            ValueRef::Real(f) => Decimal::from_str(&f.to_string()).map_err(|_| conv_err("ungültiger Betrag")),
            other => {
                let s = as_text(other)?;
                if s.trim().is_empty() {
                    return Ok(Decimal::ZERO);
                }
                Decimal::from_str(s.trim()).map_err(|_| conv_err(format!("ungültiger Betrag '{s}'")))
            }
        }
    }
    fn to_value(&self) -> Value {
        Value::Text(self.normalize().to_string())
    }
}

impl Field for NaiveDate {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        let s = as_text(v)?;
        NaiveDate::parse_from_str(s.get(..10).unwrap_or(&s), "%Y-%m-%d")
            .map_err(|_| conv_err(format!("ungültiges Datum '{s}'")))
    }
    fn to_value(&self) -> Value {
        Value::Text(self.format("%Y-%m-%d").to_string())
    }
}

impl Field for NaiveDateTime {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        let s = as_text(v)?;
        NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").map_err(|_| conv_err(format!("ungültiger Zeitstempel '{s}'")))
    }
    fn to_value(&self) -> Value {
        Value::Text(self.format("%Y-%m-%d %H:%M:%S").to_string())
    }
}

impl<T: Field> Field for Option<T> {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        match v {
            ValueRef::Null => Ok(None),
            ValueRef::Text(t) if t.is_empty() => Ok(None),
            other => T::from_ref(other).map(Some),
        }
    }
    fn to_value(&self) -> Value {
        match self {
            Some(v) => v.to_value(),
            None => Value::Null,
        }
    }
}

impl<T: Serialize + DeserializeOwned> Field for Vec<T> {
    fn from_ref(v: ValueRef<'_>) -> rusqlite::Result<Self> {
        let s = as_text(v)?;
        if s.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&s).map_err(|e| conv_err(format!("ungültiges JSON: {e}")))
    }
    fn to_value(&self) -> Value {
        Value::Text(serde_json::to_string(self).unwrap_or_else(|_| "[]".into()))
    }
}

/// Implements [`Field`] for a struct that is stored as JSON text.
#[macro_export]
macro_rules! json_field {
    ($t:ty) => {
        impl $crate::db::Field for $t {
            fn from_ref(v: rusqlite::types::ValueRef<'_>) -> rusqlite::Result<Self> {
                let s = <String as $crate::db::Field>::from_ref(v)?;
                if s.trim().is_empty() {
                    return Ok(Default::default());
                }
                serde_json::from_str(&s).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
                })
            }
            fn to_value(&self) -> rusqlite::types::Value {
                rusqlite::types::Value::Text(serde_json::to_string(self).unwrap_or_default())
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

pub trait Entity: Sized {
    const TABLE: &'static str;
    const COLUMNS: &'static [&'static str];
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self>;
    fn values(&self) -> Vec<Value>;
    fn id(&self) -> i64;
    fn set_id(&mut self, id: i64);
}

/// Declares a table-backed struct. The first field must be `id: i64`.
#[macro_export]
macro_rules! entity {
    (
        $(#[$smeta:meta])*
        pub struct $name:ident in $table:literal {
            $( $(#[$fmeta:meta])* pub $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, ts_rs::TS)]
        #[serde(default)]
        #[ts(export)]
        pub struct $name { $( $(#[$fmeta])* pub $field: $ty ),* }

        impl $crate::db::Entity for $name {
            const TABLE: &'static str = $table;
            const COLUMNS: &'static [&'static str] = &[$(stringify!($field)),*];
            fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
                Ok(Self { $( $field: <$ty as $crate::db::Field>::get(row, stringify!($field))? ),* })
            }
            fn values(&self) -> Vec<rusqlite::types::Value> {
                vec![$( $crate::db::Field::to_value(&self.$field) ),*]
            }
            fn id(&self) -> i64 { self.id }
            fn set_id(&mut self, id: i64) { self.id = id; }
        }
    };
}

pub fn insert<T: Entity>(conn: &Connection, item: &mut T) -> Result<i64> {
    let values = item.values();
    let (cols, vals): (Vec<&str>, Vec<Value>) = if item.id() > 0 {
        (T::COLUMNS.to_vec(), values)
    } else {
        (T::COLUMNS[1..].to_vec(), values.into_iter().skip(1).collect())
    };
    let placeholders = (1..=cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    let sql = format!("INSERT INTO {} ({}) VALUES ({})", T::TABLE, cols.join(", "), placeholders);
    conn.execute(&sql, params_from_iter(vals.iter()))
        .with_context(|| format!("Einfügen in {} fehlgeschlagen", T::TABLE))?;
    let id = conn.last_insert_rowid();
    item.set_id(id);
    Ok(id)
}

pub fn update<T: Entity>(conn: &Connection, item: &T) -> Result<()> {
    let values = item.values();
    let sets = T::COLUMNS[1..]
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{c} = ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("UPDATE {} SET {} WHERE id = ?{}", T::TABLE, sets, T::COLUMNS.len());
    let mut params: Vec<Value> = values.into_iter().skip(1).collect();
    params.push(Value::Integer(item.id()));
    conn.execute(&sql, params_from_iter(params.iter()))
        .with_context(|| format!("Aktualisieren von {} fehlgeschlagen", T::TABLE))?;
    Ok(())
}

pub fn find<T: Entity>(conn: &Connection, id: i64) -> Result<Option<T>> {
    let sql = format!("SELECT * FROM {} WHERE id = ?1", T::TABLE);
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(row) => Ok(Some(T::from_row(row)?)),
        None => Ok(None),
    }
}

pub fn get<T: Entity>(conn: &Connection, id: i64) -> Result<T> {
    find(conn, id)?.ok_or_else(|| crate::error::NotFound(format!("{} #{id} nicht gefunden", T::TABLE)).into())
}

/// `SELECT * FROM table <suffix>` with positional parameters.
pub fn query<T: Entity>(conn: &Connection, suffix: &str, params: &[Value]) -> Result<Vec<T>> {
    let sql = format!("SELECT * FROM {} {}", T::TABLE, suffix);
    let mut stmt = conn.prepare_cached(&sql).with_context(|| format!("SQL: {sql}"))?;
    let rows = stmt.query_map(params_from_iter(params.iter()), |r| T::from_row(r))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn query_one<T: Entity>(conn: &Connection, suffix: &str, params: &[Value]) -> Result<Option<T>> {
    Ok(query::<T>(conn, suffix, params)?.into_iter().next())
}

pub fn delete<T: Entity>(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(&format!("DELETE FROM {} WHERE id = ?1", T::TABLE), [id])?;
    Ok(())
}

/// Helper for building parameter lists.
pub fn v<T: Field>(x: T) -> Value {
    x.to_value()
}

pub fn scalar_i64(conn: &Connection, sql: &str, params: &[Value]) -> Result<i64> {
    Ok(conn.query_row(sql, params_from_iter(params.iter()), |r| r.get::<_, Option<i64>>(0))?.unwrap_or(0))
}

pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).ok())
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    const OLD: &str = "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);\n\
                       CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);";
    const NEW: &str = "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);\n\
                       CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, note TEXT NOT NULL DEFAULT '', size INTEGER NOT NULL);\n\
                       CREATE INDEX IF NOT EXISTS idx_items_note ON items(note);\n\
                       CREATE TABLE IF NOT EXISTS tags (id INTEGER PRIMARY KEY, color TEXT NOT NULL DEFAULT 'rot');";
    const STEPS: &[&str] = &["", "UPDATE items SET note = 'migriert';"];

    /// Tables and their columns.
    fn shape(conn: &Connection) -> BTreeMap<String, BTreeSet<String>> {
        let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'").unwrap();
        let tables: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        tables.into_iter().map(|t| (t.clone(), columns(conn, &t).unwrap().into_iter().map(|c| c.name).collect())).collect()
    }

    fn reference() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c
    }

    #[test]
    fn current_schema_is_created_fresh() {
        let conn = open_memory();
        assert_eq!(schema_version(&conn), Some(SCHEMA_VERSION));
    }

    #[test]
    fn older_databases_are_completed_and_migrated() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_with(&conn, OLD, &[]).unwrap();
        conn.execute("INSERT INTO items (name) VALUES ('alt')", []).unwrap();
        migrate_with(&conn, NEW, STEPS).unwrap();
        assert_eq!(schema_version(&conn), Some(3));
        let (note, size): (String, i64) = conn.query_row("SELECT note, size FROM items", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((note.as_str(), size), ("migriert", 0), "NOT NULL column without default gets the empty value");
        conn.execute("INSERT INTO tags DEFAULT VALUES", []).unwrap();
        // Running again changes nothing.
        migrate_with(&conn, NEW, STEPS).unwrap();
        assert_eq!(schema_version(&conn), Some(3));
    }

    #[test]
    fn databases_without_version_are_version_1() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(OLD).unwrap();
        conn.execute_batch("DROP TABLE meta;").unwrap();
        migrate_with(&conn, NEW, STEPS).unwrap();
        assert_eq!(schema_version(&conn), Some(3));
        assert!(columns(&conn, "items").unwrap().iter().any(|c| c.name == "note"));
    }

    #[test]
    fn newer_databases_are_refused() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_with(&conn, NEW, STEPS).unwrap();
        let err = migrate_with(&conn, OLD, &[]).unwrap_err().to_string();
        assert!(err.contains("neueren Sorobo-Version"), "{err}");
    }

    #[test]
    fn failed_migration_changes_nothing() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_with(&conn, OLD, &[]).unwrap();
        assert!(migrate_with(&conn, NEW, &["", "SELECT * FROM fehlt;"]).is_err());
        assert_eq!(schema_version(&conn), Some(1));
        assert!(!table_exists(&conn, "tags").unwrap(), "added table rolled back");
        assert_eq!(columns(&conn, "items").unwrap().len(), 2, "added columns rolled back");
    }

    /// Schemas of the released versions (0.9.0 equals 0.9.1 except for the licence header).
    /// (release, its schema version, its schema.sql)
    const RELEASED: &[(&str, i64, &str)] = &[
        ("0.9.1", 1, include_str!("testdata/schema-0.9.1.sql")),
        ("0.9.2", 1, include_str!("testdata/schema-0.9.2.sql")),
        ("0.9.3", 1, include_str!("testdata/schema-0.9.3.sql")),
        ("0.10.0", 3, include_str!("testdata/schema-0.10.0.sql")),
    ];

    #[test]
    fn every_released_version_is_migrated_with_its_data() {
        for (release, version, schema) in RELEASED {
            let conn = Connection::open_in_memory().unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            conn.execute_batch(schema).unwrap();
            conn.execute("INSERT INTO meta (key, value) VALUES ('schema_version', ?1)", [version.to_string()]).unwrap();
            conn.execute_batch(
                "INSERT INTO contacts (id, company) VALUES (1, 'Kunde GmbH');
                 INSERT INTO documents (id, doc_type, contact_id, date, number, gross_total) VALUES (1, 'invoice', 1, '2026-01-31', 'RE-1', '119');",
            )
            .unwrap();
            migrate(&conn).unwrap_or_else(|e| panic!("{release}: {e:#}"));
            assert_eq!(schema_version(&conn), Some(SCHEMA_VERSION), "{release}");
            assert_eq!(shape(&conn), shape(&reference()), "{release}");
            let (number, method): (String, String) = conn.query_row("SELECT number, payment_method FROM documents WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
            assert_eq!((number.as_str(), method.as_str()), ("RE-1", ""), "{release}");
            conn.execute("INSERT INTO recurring_invoices (template_id, next_date, auto_finalize) VALUES (1, '2026-02-28', 1)", []).unwrap();
            // The entities read the migrated rows.
            assert_eq!(query::<crate::models::Document>(&conn, "", &[]).unwrap().len(), 1, "{release}");
            assert_eq!(query::<crate::models::RecurringInvoice>(&conn, "", &[]).unwrap().len(), 1, "{release}");
        }
    }

    #[test]
    fn every_partial_state_is_completed() {
        let full = shape(&reference());
        // Each table missing (older versions lack the newer tables) …
        for table in full.keys().filter(|t| *t != "meta") {
            let conn = reference();
            conn.execute_batch(&format!("DROP TABLE \"{table}\"; INSERT OR REPLACE INTO meta VALUES ('schema_version', '1');")).unwrap();
            migrate(&conn).unwrap_or_else(|e| panic!("without {table}: {e:#}"));
            assert_eq!(shape(&conn), full, "without {table}");
        }
        // … and each column missing that SQLite can drop (columns are only ever added).
        for (table, cols) in full.iter().filter(|(t, _)| *t != "meta") {
            for col in cols {
                let conn = reference();
                if conn.execute_batch(&format!("ALTER TABLE \"{table}\" DROP COLUMN \"{col}\"")).is_err() {
                    continue;
                }
                conn.execute_batch("INSERT OR REPLACE INTO meta VALUES ('schema_version', '1');").unwrap();
                migrate(&conn).unwrap_or_else(|e| panic!("without {table}.{col}: {e:#}"));
                assert_eq!(shape(&conn), full, "without {table}.{col}");
            }
        }
    }

    #[test]
    fn a_backup_is_made_before_an_upgrade() {
        let dir = std::env::temp_dir().join(format!("sorobo-db-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sorobo.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(RELEASED[0].2).unwrap();
            conn.execute_batch("INSERT INTO meta (key, value) VALUES ('schema_version', '1');").unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(schema_version(&conn), Some(SCHEMA_VERSION));
        let backups: Vec<String> = std::fs::read_dir(dir.join("backups")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert_eq!(backups.len(), 1, "{backups:?}");
        assert!(backups[0].starts_with(&format!("Sorobo-vor-Update-Schema1-{SCHEMA_VERSION}_")));
        let old = Connection::open(dir.join("backups").join(&backups[0])).unwrap();
        assert_eq!(schema_version(&old), Some(1), "the backup is the unchanged database");
        drop((conn, old));
        // Opening the current version again needs no backup.
        open(&path).unwrap();
        assert_eq!(std::fs::read_dir(dir.join("backups")).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
