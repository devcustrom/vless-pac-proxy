//! SQLite-хранилище истории сайтов и проверок.
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::route_auto::{ProbeResult, RouteDecision};
use crate::routing::AutoChannel;
use crate::state::CheckEntry;

const DB_FILE: &str = "wireproxy.db";
const SCHEMA_VERSION: i64 = 2;

pub const DOMAIN_EXTRA: &str = "extra";
pub const DOMAIN_DIRECT: &str = "direct";
pub const DOMAIN_VPN_ONLY: &str = "vpn_only";

#[derive(Debug, Clone, Default)]
pub struct XrayProfileRow {
    pub slug: String,
    pub name: String,
    pub config_json: String,
    pub protocol: String,
    pub address: String,
    pub port: u16,
    pub dynamic: bool,
    pub user_selected: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomainSets {
    pub extra: Vec<String>,
    pub direct: Vec<String>,
    pub vpn_only: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct VisitedRow {
    pub domain: String,
    pub count: u64,
    pub last: i64,
    pub checked: bool,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = crate::paths::in_dir(dir, DB_FILE)?;
        let conn = Connection::open(path).context("открытие wireproxy.db")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.schema()?;
        Ok(store)
    }

    pub fn memory() -> Self {
        let conn = Connection::open_in_memory().expect("sqlite memory");
        let store = Self {
            conn: Mutex::new(conn),
        };
        let _ = store.schema();
        store
    }

    fn schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS visited (
                domain     TEXT PRIMARY KEY,
                count      INTEGER NOT NULL DEFAULT 1,
                first_seen INTEGER NOT NULL,
                last_seen  INTEGER NOT NULL,
                checked_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS visited_last ON visited(last_seen DESC);
             CREATE TABLE IF NOT EXISTS checks (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                domain           TEXT NOT NULL,
                direct           TEXT NOT NULL,
                vpn              TEXT NOT NULL,
                zapret           TEXT NOT NULL,
                zapret_running   INTEGER NOT NULL DEFAULT 0,
                zapret_supported INTEGER NOT NULL DEFAULT 0,
                verdict          TEXT NOT NULL,
                at               TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS checks_domain ON checks(domain, id DESC);
             CREATE TABLE IF NOT EXISTS auto_routes (
                domain         TEXT PRIMARY KEY,
                route          TEXT NOT NULL,
                available      INTEGER NOT NULL,
                latency_ms     INTEGER NOT NULL,
                direct_code    TEXT NOT NULL,
                direct_ms      INTEGER NOT NULL,
                wireguard_code TEXT,
                wireguard_ms   INTEGER,
                vless_code     TEXT,
                vless_ms       INTEGER,
                via_zapret     INTEGER NOT NULL,
                checked_at     INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS auto_routes_checked ON auto_routes(checked_at);",
        )?;

        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < SCHEMA_VERSION {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version    INTEGER PRIMARY KEY,
                    applied_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS meta (
                    key   TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS settings (
                    id         INTEGER PRIMARY KEY CHECK(id = 1),
                    json       TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS domains (
                    domain     TEXT NOT NULL,
                    kind       TEXT NOT NULL,
                    position   INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY(domain, kind)
                 );
                 CREATE INDEX IF NOT EXISTS domains_kind_pos ON domains(kind, position);
                 CREATE TABLE IF NOT EXISTS local_domains (
                    domain     TEXT PRIMARY KEY,
                    port       INTEGER NOT NULL,
                    position   INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS subscriptions (
                    id           INTEGER PRIMARY KEY AUTOINCREMENT,
                    url          TEXT NOT NULL UNIQUE,
                    name         TEXT NOT NULL DEFAULT '',
                    enabled      INTEGER NOT NULL DEFAULT 1,
                    last_refresh INTEGER,
                    last_error   TEXT,
                    created_at   INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS wg_profiles (
                    id         INTEGER PRIMARY KEY AUTOINCREMENT,
                    slug       TEXT NOT NULL UNIQUE,
                    name       TEXT NOT NULL,
                    config     TEXT NOT NULL,
                    active     INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE UNIQUE INDEX IF NOT EXISTS wg_profiles_active
                    ON wg_profiles(active) WHERE active = 1;
                 CREATE TABLE IF NOT EXISTS xray_profiles (
                    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
                    slug                   TEXT NOT NULL UNIQUE,
                    name                   TEXT NOT NULL,
                    config_json            TEXT NOT NULL,
                    protocol               TEXT,
                    address                TEXT,
                    port                   INTEGER,
                    dynamic                INTEGER NOT NULL DEFAULT 0,
                    user_selected          INTEGER NOT NULL DEFAULT 0,
                    active                 INTEGER NOT NULL DEFAULT 0,
                    source_subscription_id INTEGER,
                    created_at             INTEGER NOT NULL,
                    updated_at             INTEGER NOT NULL
                 );
                 CREATE UNIQUE INDEX IF NOT EXISTS xray_profiles_active
                    ON xray_profiles(active) WHERE active = 1;
                 CREATE TABLE IF NOT EXISTS pool_state (
                    id          INTEGER PRIMARY KEY CHECK(id = 1),
                    active_link TEXT,
                    entries_json TEXT NOT NULL DEFAULT '[]',
                    updated_at  INTEGER NOT NULL
                 );",
            )?;
            let now = chrono::Utc::now().timestamp();
            conn.execute(
                "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(?1, ?2)",
                params![SCHEMA_VERSION, now],
            )?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    pub fn settings_json(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT json FROM settings WHERE id = 1")?;
        let mut rows = stmt.query([])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
    }

    pub fn save_settings_json(&self, json: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings(id, json, updated_at) VALUES(1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET json = excluded.json,
                                          updated_at = excluded.updated_at",
            params![json, chrono::Utc::now().timestamp()],
        )?;
        Ok(())
    }

    pub fn pool_state_json(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT entries_json FROM pool_state WHERE id = 1")?;
        let mut rows = stmt.query([])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
    }

    pub fn save_pool_state_json(&self, json: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO pool_state(id, entries_json, updated_at) VALUES(1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET entries_json = excluded.entries_json,
                                          updated_at = excluded.updated_at",
            params![json, chrono::Utc::now().timestamp()],
        )?;
        Ok(())
    }

    pub fn domain_sets(&self) -> Result<DomainSets> {
        let conn = self.conn.lock().unwrap();
        load_domain_sets(&conn)
    }

    /// Atomically reads all three user domain lists, applies `change` and
    /// replaces the stored rows in one transaction.
    pub fn mutate_domain_sets<F>(&self, change: F) -> Result<DomainSets>
    where
        F: FnOnce(&mut DomainSets),
    {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut sets = load_domain_sets(&tx)?;
        change(&mut sets);
        normalize_domain_sets(&mut sets);
        let now = chrono::Utc::now().timestamp();
        tx.execute("DELETE FROM domains", [])?;
        for (kind, list) in [
            (DOMAIN_EXTRA, &sets.extra),
            (DOMAIN_DIRECT, &sets.direct),
            (DOMAIN_VPN_ONLY, &sets.vpn_only),
        ] {
            for (position, domain) in list.iter().enumerate() {
                tx.execute(
                    "INSERT INTO domains(domain, kind, position, created_at)
                     VALUES(?1, ?2, ?3, ?4)",
                    params![domain, kind, position as i64, now],
                )?;
            }
        }
        tx.commit()?;
        Ok(sets)
    }

    pub fn replace_domain_sets(&self, sets: &DomainSets) -> Result<()> {
        self.mutate_domain_sets(|current| *current = sets.clone())?;
        Ok(())
    }

    pub fn local_domains(&self) -> Result<Vec<crate::net::local_proxy::LocalDomain>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT domain, port FROM local_domains ORDER BY position, created_at, domain",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::net::local_proxy::LocalDomain {
                domain: row.get(0)?,
                port: row.get::<_, i64>(1)?.clamp(1, 65535) as u16,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn replace_local_domains(
        &self,
        list: &[crate::net::local_proxy::LocalDomain],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = chrono::Utc::now().timestamp();
        tx.execute("DELETE FROM local_domains", [])?;
        for (position, item) in list.iter().enumerate() {
            tx.execute(
                "INSERT INTO local_domains(domain, port, position, created_at)
                 VALUES(?1, ?2, ?3, ?4)",
                params![item.domain, item.port as i64, position as i64, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn subscription_urls(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT url FROM subscriptions ORDER BY id")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn replace_subscription_urls(&self, urls: &[String]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = chrono::Utc::now().timestamp();
        let mut seen = HashSet::new();
        tx.execute("DELETE FROM subscriptions", [])?;
        for url in urls {
            let url = url.trim();
            if url.is_empty() || !seen.insert(url.to_string()) {
                continue;
            }
            tx.execute(
                "INSERT INTO subscriptions(url, name, created_at) VALUES(?1, '', ?2)",
                params![url, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn wg_profiles(&self) -> Result<Vec<(String, String, String, bool)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT slug, name, config, active FROM wg_profiles ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, i64>(3)? != 0,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn wg_profile(&self, slug: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT config FROM wg_profiles WHERE slug = ?1")?;
        let mut rows = stmt.query(params![slug])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
    }

    pub fn active_wg_profile(&self) -> Result<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT slug, config FROM wg_profiles WHERE active = 1 LIMIT 1")?;
        let mut rows = stmt.query([])?;
        Ok(rows
            .next()?
            .map(|row| -> rusqlite::Result<(String, String)> { Ok((row.get(0)?, row.get(1)?)) })
            .transpose()?)
    }

    pub fn insert_wg_profile(&self, slug: &str, name: &str, config: &str) -> Result<bool> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM wg_profiles", [], |row| row.get(0))?;
        let now = chrono::Utc::now().timestamp();
        tx.execute(
            "INSERT INTO wg_profiles(slug, name, config, active, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(slug) DO UPDATE SET
                 name = excluded.name,
                 config = excluded.config,
                 updated_at = excluded.updated_at",
            params![slug, name, config, (count == 0) as i64, now],
        )?;
        tx.commit()?;
        Ok(count == 0)
    }

    pub fn activate_wg_profile(&self, slug: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM wg_profiles WHERE slug = ?1",
            params![slug],
            |row| row.get(0),
        )?;
        if exists == 0 {
            anyhow::bail!("профиль {slug} не найден");
        }
        tx.execute("UPDATE wg_profiles SET active = 0 WHERE active = 1", [])?;
        tx.execute(
            "UPDATE wg_profiles SET active = 1 WHERE slug = ?1",
            params![slug],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_wg_profile(&self, slug: &str) -> Result<(bool, Option<String>)> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let was_active: i64 = tx.query_row(
            "SELECT active FROM wg_profiles WHERE slug = ?1",
            params![slug],
            |row| row.get(0),
        )?;
        tx.execute("DELETE FROM wg_profiles WHERE slug = ?1", params![slug])?;
        let mut next = None;
        if was_active != 0 {
            let candidate: Option<String> = tx
                .query_row(
                    "SELECT slug FROM wg_profiles ORDER BY id LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();
            if let Some(slug) = candidate.as_deref() {
                tx.execute(
                    "UPDATE wg_profiles SET active = 1 WHERE slug = ?1",
                    params![slug],
                )?;
            }
            next = candidate;
        }
        tx.commit()?;
        Ok((was_active != 0, next))
    }

    pub fn xray_profiles(&self) -> Result<Vec<XrayProfileRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT slug, name, config_json, protocol, address, port,
                    dynamic, user_selected, active
             FROM xray_profiles ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(XrayProfileRow {
                slug: row.get(0)?,
                name: row.get(1)?,
                config_json: row.get(2)?,
                protocol: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                address: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                port: row.get::<_, Option<i64>>(5)?.unwrap_or(0).clamp(0, 65535) as u16,
                dynamic: row.get::<_, i64>(6)? != 0,
                user_selected: row.get::<_, i64>(7)? != 0,
                active: row.get::<_, i64>(8)? != 0,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn xray_profile(&self, slug: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT config_json FROM xray_profiles WHERE slug = ?1")?;
        let mut rows = stmt.query(params![slug])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
    }

    pub fn active_xray_profile(&self) -> Result<Option<XrayProfileRow>> {
        Ok(self
            .xray_profiles()?
            .into_iter()
            .find(|profile| profile.active))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_xray_profile(
        &self,
        slug: &str,
        name: &str,
        config_json: &str,
        protocol: &str,
        address: &str,
        port: u16,
        dynamic: bool,
    ) -> Result<bool> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let count: i64 =
            tx.query_row("SELECT COUNT(*) FROM xray_profiles", [], |row| row.get(0))?;
        let now = chrono::Utc::now().timestamp();
        tx.execute(
            "INSERT INTO xray_profiles(
                slug, name, config_json, protocol, address, port, dynamic,
                active, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT(slug) DO UPDATE SET
                name = excluded.name,
                config_json = excluded.config_json,
                protocol = excluded.protocol,
                address = excluded.address,
                port = excluded.port,
                dynamic = excluded.dynamic,
                updated_at = excluded.updated_at",
            params![
                slug,
                name,
                config_json,
                protocol,
                address,
                port as i64,
                dynamic as i64,
                (count == 0) as i64,
                now
            ],
        )?;
        tx.commit()?;
        Ok(count == 0)
    }

    pub fn activate_xray_profile(&self, slug: &str, user_selected: bool) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM xray_profiles WHERE slug = ?1",
            params![slug],
            |row| row.get(0),
        )?;
        if exists == 0 {
            anyhow::bail!("профиль {slug} не найден");
        }
        tx.execute("UPDATE xray_profiles SET active = 0 WHERE active = 1", [])?;
        tx.execute(
            "UPDATE xray_profiles SET active = 1, user_selected = ?2 WHERE slug = ?1",
            params![slug, user_selected as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn clear_active_xray(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE xray_profiles SET active = 0 WHERE active = 1", [])?;
        Ok(())
    }

    pub fn delete_xray_profile(&self, slug: &str) -> Result<(bool, Option<String>)> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let was_active: i64 = tx.query_row(
            "SELECT active FROM xray_profiles WHERE slug = ?1",
            params![slug],
            |row| row.get(0),
        )?;
        tx.execute("DELETE FROM xray_profiles WHERE slug = ?1", params![slug])?;
        let mut next = None;
        if was_active != 0 {
            let candidate: Option<String> = tx
                .query_row(
                    "SELECT slug FROM xray_profiles ORDER BY id LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();
            if let Some(slug) = candidate.as_deref() {
                tx.execute(
                    "UPDATE xray_profiles SET active = 1 WHERE slug = ?1",
                    params![slug],
                )?;
            }
            next = candidate;
        }
        tx.commit()?;
        Ok((was_active != 0, next))
    }

    pub fn note_visit(&self, domain: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO visited(domain, count, first_seen, last_seen)
             VALUES(?1, 1, ?2, ?2)
             ON CONFLICT(domain) DO UPDATE SET
               count = count + 1,
               last_seen = excluded.last_seen",
            params![domain, now],
        )?;
        Ok(())
    }

    pub fn visited(&self, cutoff: i64, limit: usize) -> Result<Vec<VisitedRow>> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "DELETE FROM visited WHERE last_seen < ?1",
            params![now - 7 * 86_400],
        );
        let _ = conn.execute(
            "DELETE FROM checks WHERE id NOT IN (SELECT id FROM checks ORDER BY id DESC LIMIT 500)",
            [],
        );
        let mut stmt = conn.prepare(
            "SELECT domain, count, last_seen, checked_at IS NOT NULL FROM visited
             WHERE last_seen >= ?1 ORDER BY last_seen DESC, domain ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![cutoff, limit as i64], |row| {
            Ok(VisitedRow {
                domain: row.get(0)?,
                count: row.get::<_, i64>(1)? as u64,
                last: row.get(2)?,
                checked: row.get::<_, i64>(3)? != 0,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn checks(&self, limit: usize) -> Result<Vec<CheckEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT domain, direct, vpn, zapret, zapret_running,
                    zapret_supported, verdict, at
             FROM checks ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(CheckEntry {
                domain: row.get(0)?,
                direct: row.get(1)?,
                vpn: row.get(2)?,
                zapret: row.get(3)?,
                zapret_running: row.get::<_, i64>(4)? != 0,
                zapret_supported: row.get::<_, i64>(5)? != 0,
                verdict: row.get(6)?,
                at: row.get(7)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn insert_check(&self, entry: &CheckEntry) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO checks(domain, direct, vpn, zapret, zapret_running,
                                zapret_supported, verdict, at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.domain,
                entry.direct,
                entry.vpn,
                entry.zapret,
                entry.zapret_running as i64,
                entry.zapret_supported as i64,
                entry.verdict,
                entry.at
            ],
        )?;
        conn.execute(
            "UPDATE visited SET checked_at = ?1 WHERE domain = ?2",
            params![now, entry.domain],
        )?;
        Ok(())
    }

    pub fn checked_domains(&self, cutoff: i64) -> Result<HashSet<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT domain FROM visited WHERE checked_at >= ?1")?;
        let rows = stmt.query_map(params![cutoff], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
    }

    pub fn mark_checked(&self, domain: &str, at: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO visited(domain, count, first_seen, last_seen, checked_at)
             VALUES(?1, 1, ?2, ?2, ?2)
             ON CONFLICT(domain) DO UPDATE SET checked_at = excluded.checked_at",
            params![domain, at],
        )?;
        Ok(())
    }

    pub fn auto_routes(&self) -> Result<HashMap<String, RouteDecision>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT domain, route, available, latency_ms, direct_code, direct_ms,
                    wireguard_code, wireguard_ms, vless_code, vless_ms, via_zapret, checked_at
             FROM auto_routes",
        )?;
        let rows = stmt.query_map([], |row| {
            let channel =
                AutoChannel::parse(&row.get::<_, String>(1)?).unwrap_or(AutoChannel::Direct);
            let opt_probe = |code: Option<String>, ms: Option<i64>| match (code, ms) {
                (Some(code), Some(ms)) => Some(ProbeResult {
                    code,
                    ms: ms.max(0) as u64,
                }),
                _ => None,
            };
            Ok((
                row.get::<_, String>(0)?,
                RouteDecision {
                    channel,
                    available: row.get::<_, i64>(2)? != 0,
                    latency_ms: row.get::<_, i64>(3)?.max(0) as u64,
                    direct: ProbeResult {
                        code: row.get(4)?,
                        ms: row.get::<_, i64>(5)?.max(0) as u64,
                    },
                    wireguard: opt_probe(row.get(6)?, row.get(7)?),
                    vless: opt_probe(row.get(8)?, row.get(9)?),
                    via_zapret: row.get::<_, i64>(10)? != 0,
                    checked_at: row.get(11)?,
                },
            ))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (domain, decision) = row?;
            out.insert(domain, decision);
        }
        Ok(out)
    }

    pub fn save_auto_route(&self, domain: &str, decision: &RouteDecision) -> Result<()> {
        let (wg_code, wg_ms) = decision
            .wireguard
            .as_ref()
            .map(|probe| (Some(probe.code.clone()), Some(probe.ms as i64)))
            .unwrap_or((None, None));
        let (vless_code, vless_ms) = decision
            .vless
            .as_ref()
            .map(|probe| (Some(probe.code.clone()), Some(probe.ms as i64)))
            .unwrap_or((None, None));
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO auto_routes(domain, route, available, latency_ms,
                 direct_code, direct_ms, wireguard_code, wireguard_ms,
                 vless_code, vless_ms, via_zapret, checked_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(domain) DO UPDATE SET
                 route = excluded.route,
                 available = excluded.available,
                 latency_ms = excluded.latency_ms,
                 direct_code = excluded.direct_code,
                 direct_ms = excluded.direct_ms,
                 wireguard_code = excluded.wireguard_code,
                 wireguard_ms = excluded.wireguard_ms,
                 vless_code = excluded.vless_code,
                 vless_ms = excluded.vless_ms,
                 via_zapret = excluded.via_zapret,
                 checked_at = excluded.checked_at",
            params![
                domain,
                decision.channel.as_str(),
                decision.available as i64,
                decision.latency_ms as i64,
                decision.direct.code,
                decision.direct.ms as i64,
                wg_code,
                wg_ms,
                vless_code,
                vless_ms,
                decision.via_zapret as i64,
                decision.checked_at
            ],
        )?;
        Ok(())
    }

    pub fn delete_auto_route(&self, domain: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM auto_routes WHERE domain = ?1", params![domain])?;
        Ok(())
    }

    pub fn prune_auto_routes(&self, keep: &HashSet<String>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let existing: Vec<String> = {
            let mut stmt = conn.prepare("SELECT domain FROM auto_routes")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for domain in existing {
            if !keep.contains(&domain) {
                conn.execute("DELETE FROM auto_routes WHERE domain = ?1", params![domain])?;
            }
        }
        Ok(())
    }

    /// Согласованный снимок БД для архива (без WAL-файлов).
    pub fn backup_to(&self, path: &Path) -> Result<()> {
        let _ = std::fs::remove_file(path);
        let conn = self.conn.lock().unwrap();
        conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()])?;
        Ok(())
    }
}

fn load_domain_sets(conn: &Connection) -> Result<DomainSets> {
    fn load_kind(conn: &Connection, kind: &str) -> Result<Vec<String>> {
        let mut stmt = conn.prepare(
            "SELECT domain FROM domains WHERE kind = ?1
             ORDER BY position, created_at, rowid",
        )?;
        let rows = stmt.query_map(params![kind], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }
    Ok(DomainSets {
        extra: load_kind(conn, DOMAIN_EXTRA)?,
        direct: load_kind(conn, DOMAIN_DIRECT)?,
        vpn_only: load_kind(conn, DOMAIN_VPN_ONLY)?,
    })
}

fn normalize_domain_sets(sets: &mut DomainSets) {
    for list in [&mut sets.extra, &mut sets.direct, &mut sets.vpn_only] {
        let mut seen = HashSet::new();
        let mut normalized = Vec::with_capacity(list.len());
        for item in list.iter() {
            let Some(domain) = crate::lists::normalize_domain(item) else {
                continue;
            };
            if seen.insert(domain.clone()) {
                normalized.push(domain);
            }
        }
        *list = normalized;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wpc-store-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn auto_routes_roundtrip() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();
        let decision = RouteDecision {
            channel: AutoChannel::Wireguard,
            available: true,
            latency_ms: 120,
            direct: ProbeResult {
                code: "403".into(),
                ms: 80,
            },
            wireguard: Some(ProbeResult {
                code: "200".into(),
                ms: 120,
            }),
            vless: None,
            via_zapret: false,
            checked_at: 42,
        };
        store.save_auto_route("chatgpt.com", &decision).unwrap();
        let loaded = store.auto_routes().unwrap();
        assert_eq!(
            loaded.get("chatgpt.com").unwrap().channel,
            AutoChannel::Wireguard
        );
        assert_eq!(loaded.get("chatgpt.com").unwrap().latency_ms, 120);
        drop(store);
        let reopened = Store::open(&dir).unwrap();
        assert!(reopened.auto_routes().unwrap().contains_key("chatgpt.com"));
        reopened
            .prune_auto_routes(&HashSet::from(["other.com".to_string()]))
            .unwrap();
        assert!(reopened.auto_routes().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_domain_updates_are_not_lost() {
        let dir = temp_dir();
        let store = std::sync::Arc::new(Store::open(&dir).unwrap());
        let mut handles = Vec::new();
        for index in 0..12 {
            let store = store.clone();
            handles.push(std::thread::spawn(move || {
                store
                    .mutate_domain_sets(|sets| {
                        sets.extra.push(format!("d{index}.test"));
                    })
                    .unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let sets = store.domain_sets().unwrap();
        assert_eq!(sets.extra.len(), 12);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn domains_and_local_domains_roundtrip() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let sets = store
            .mutate_domain_sets(|sets| {
                sets.extra = vec!["Example.COM".into(), "example.com".into(), "b.test".into()];
                sets.direct = vec!["lan.test".into()];
                sets.vpn_only = vec!["chatgpt.com".into()];
            })
            .unwrap();
        assert_eq!(sets.extra, vec!["example.com", "b.test"]);
        assert_eq!(store.domain_sets().unwrap(), sets);
        let local = vec![
            crate::net::local_proxy::LocalDomain {
                domain: "panel.localhost".into(),
                port: 8080,
            },
            crate::net::local_proxy::LocalDomain {
                domain: "app.test".into(),
                port: 3000,
            },
        ];
        store.replace_local_domains(&local).unwrap();
        drop(store);
        let reopened = Store::open(&dir).unwrap();
        assert_eq!(reopened.domain_sets().unwrap().vpn_only, sets.vpn_only);
        assert_eq!(reopened.local_domains().unwrap(), local);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn visits_checks_and_backup() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();
        store.note_visit("example.com").unwrap();
        store.note_visit("example.com").unwrap();
        store.note_visit("other.test").unwrap();
        let rows = store.visited(0, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].count, 2);
        let entry = CheckEntry {
            domain: "example.com".into(),
            direct: "000".into(),
            vpn: "200".into(),
            zapret: "n/a".into(),
            zapret_running: false,
            zapret_supported: true,
            verdict: "vpn-helps".into(),
            at: chrono::Utc::now().to_rfc3339(),
        };
        store.insert_check(&entry).unwrap();
        assert_eq!(store.checks(10).unwrap()[0].domain, "example.com");
        assert!(store.checked_domains(0).unwrap().contains("example.com"));
        let backup = dir.join("snapshot.db");
        store.backup_to(&backup).unwrap();
        assert!(backup.is_file());
        drop(store);
        let reopened = Store::open(&dir).unwrap();
        assert_eq!(reopened.visited(0, 10).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
