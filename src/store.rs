//! SQLite-хранилище истории сайтов и проверок.
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::state::CheckEntry;

const DB_FILE: &str = "wireproxy.db";

#[derive(Debug, Clone)]
pub struct VisitedRow {
    pub domain: String,
    pub count: u64,
    pub last: i64,
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
             CREATE INDEX IF NOT EXISTS checks_domain ON checks(domain, id DESC);",
        )?;
        Ok(())
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
            "SELECT domain, count, last_seen FROM visited
             WHERE last_seen >= ?1 ORDER BY last_seen DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![cutoff, limit as i64], |row| {
            Ok(VisitedRow {
                domain: row.get(0)?,
                count: row.get::<_, i64>(1)? as u64,
                last: row.get(2)?,
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

    /// Согласованный снимок БД для архива (без WAL-файлов).
    pub fn backup_to(&self, path: &Path) -> Result<()> {
        let _ = std::fs::remove_file(path);
        let conn = self.conn.lock().unwrap();
        conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()])?;
        Ok(())
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
