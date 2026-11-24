//! The register's and the books' one store: a SQLite database, `<config dir>/stablehand.db`,
//! the config dir `$STABLEHAND_CONFIG_DIR` or `~/.config/stablehand`, guarded
//! as sesame keeps its own: the directory 0700, the file 0600. It holds no
//! secret: a portable login's token is sesame's.
use crate::account::{Account, Engine, Off};
use crate::books::{Entry, What};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The schema, one step per `PRAGMA user_version`: append a step, never edit one.
const MIGRATIONS: &[&str] = &[
    "
    CREATE TABLE accounts (
        name TEXT PRIMARY KEY,
        engine TEXT NOT NULL,
        machine TEXT,
        sessions INTEGER,
        expires TEXT,
        home TEXT,
        off_by TEXT,
        off_at TEXT,
        off_reason TEXT
    );
",
    "
    -- The books: one row per reading (its windows, JSON), failed try (why)
    -- or limit (its window and reset, where said).
    CREATE TABLE books (
        account TEXT NOT NULL,
        at INTEGER NOT NULL,
        kind TEXT NOT NULL CHECK (kind IN ('read', 'failed', 'limit')),
        windows TEXT,
        failed TEXT,
        limit_window TEXT,
        limit_resets_at INTEGER
    );
    CREATE INDEX books_by_account ON books (account, at);
",
];

/// How long a process waits for another's write.
const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

const COLUMNS: &str = "name, engine, machine, sessions, expires, home, off_by, off_at, off_reason";

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

/// `$STABLEHAND_CONFIG_DIR`, else `~/.config/stablehand`.
fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("STABLEHAND_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".config").join("stablehand")
}

/// Makes the directory 0700 if missing and the file 0600, tightening a wider one.
fn prepare(path: &Path) -> Result<(), String> {
    let at = |e: std::io::Error, p: &Path| format!("{}: {e}", p.display());
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| at(e, dir))?;
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(|e| at(e, path))?;
    let mode = file
        .metadata()
        .map_err(|e| at(e, path))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| at(e, path))?;
    }
    Ok(())
}

fn engine_of(row: &Row) -> rusqlite::Result<Engine> {
    let name: String = row.get(1)?;
    clap::ValueEnum::from_str(&name, false).map_err(|e: String| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, e.into())
    })
}

fn engine_name(e: Engine) -> String {
    let value = clap::ValueEnum::to_possible_value(&e).expect("every engine is a value");
    value.get_name().to_string()
}

fn account_of(row: &Row) -> rusqlite::Result<Account> {
    let off = match (row.get(6)?, row.get(7)?, row.get(8)?) {
        (Some(by), Some(at), Some(reason)) => Some(Off { by, at, reason }),
        _ => None,
    };
    Ok(Account {
        name: row.get(0)?,
        engine: engine_of(row)?,
        machine: row.get(2)?,
        sessions: row.get(3)?,
        expires: row.get(4)?,
        home: row.get(5)?,
        off,
    })
}

impl Store {
    pub fn open() -> Result<Store, String> {
        let path = config_dir().join("stablehand.db");
        prepare(&path)?;
        let conn = Connection::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let store = Store { conn, path };
        // No WAL: the rollback journal SQLite makes takes the database's 0600.
        store
            .conn
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(|e| store.err(e))?;
        store.migrate()?;
        Ok(store)
    }

    fn err(&self, e: rusqlite::Error) -> String {
        format!("{}: {e}", self.path.display())
    }

    fn migrate(&self) -> Result<(), String> {
        let at: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| self.err(e))?;
        for (i, step) in MIGRATIONS.iter().enumerate().skip(at as usize) {
            self.conn
                .execute_batch(&format!(
                    "BEGIN; {step} PRAGMA user_version = {}; COMMIT;",
                    i + 1
                ))
                .map_err(|e| self.err(e))?;
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Option<Account>, String> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM accounts WHERE name = ?1"),
                [name],
                account_of,
            )
            .optional()
            .map_err(|e| self.err(e))
    }

    /// Every account, by name.
    pub fn list(&self) -> Result<Vec<Account>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM accounts ORDER BY name"))
            .map_err(|e| self.err(e))?;
        stmt.query_map([], account_of)
            .and_then(|rows| rows.collect())
            .map_err(|e| self.err(e))
    }

    /// Adds `account`; false, writing nothing, when one of its name is kept.
    pub fn insert(&self, account: &Account) -> Result<bool, String> {
        self.write("INSERT OR IGNORE INTO", account)
    }

    /// Replaces the account of `account`'s name with it.
    pub fn put(&self, account: &Account) -> Result<(), String> {
        self.write("REPLACE INTO", account).map(|_| ())
    }

    fn write(&self, verb: &str, a: &Account) -> Result<bool, String> {
        let off = a.off.as_ref();
        self.conn
            .execute(
                &format!("{verb} accounts ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"),
                params![
                    a.name,
                    engine_name(a.engine),
                    a.machine,
                    a.sessions,
                    a.expires,
                    a.home,
                    off.map(|o| &o.by),
                    off.map(|o| &o.at),
                    off.map(|o| &o.reason),
                ],
            )
            .map(|n| n == 1)
            .map_err(|e| self.err(e))
    }

    /// Removes the account `name` and its books; false when there was no account.
    pub fn remove(&self, name: &str) -> Result<bool, String> {
        let tx = self.conn.unchecked_transaction().map_err(|e| self.err(e))?;
        tx.execute("DELETE FROM books WHERE account = ?1", [name])
            .map_err(|e| self.err(e))?;
        let removed = tx
            .execute("DELETE FROM accounts WHERE name = ?1", [name])
            .map_err(|e| self.err(e))?;
        tx.commit().map_err(|e| self.err(e))?;
        Ok(removed == 1)
    }
}

fn entry_of(row: &Row) -> rusqlite::Result<Entry> {
    let at: i64 = row.get(0)?;
    let kind: String = row.get(1)?;
    let what = match kind.as_str() {
        "read" => {
            let text: String = row.get(2)?;
            What::Read(serde_json::from_str(&text).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
            })?)
        }
        "failed" => What::Failed(row.get(3)?),
        _ => What::Limit {
            window: row.get(4)?,
            resets_at: row.get::<_, Option<i64>>(5)?.map(|r| r as u64),
        },
    };
    Ok(Entry {
        at: at as u64,
        what,
    })
}

impl Store {
    /// Books `entry` in `account`'s books.
    pub fn book(&self, account: &str, entry: &Entry) -> Result<(), String> {
        let (kind, windows, failed, window, resets_at) = match &entry.what {
            What::Read(w) => (
                "read",
                Some(serde_json::to_string(w).map_err(|e| e.to_string())?),
                None,
                None,
                None,
            ),
            What::Failed(why) => ("failed", None, Some(why.as_str()), None, None),
            What::Limit { window, resets_at } => (
                "limit",
                None,
                None,
                window.as_deref(),
                resets_at.map(|r| r as i64),
            ),
        };
        self.conn
            .execute(
                "INSERT INTO books (account, at, kind, windows, failed, limit_window, limit_resets_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![account, entry.at as i64, kind, windows, failed, window, resets_at],
            )
            .map(|_| ())
            .map_err(|e| self.err(e))
    }

    /// `account`'s books, oldest first.
    pub fn entries(&self, account: &str) -> Result<Vec<Entry>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT at, kind, windows, failed, limit_window, limit_resets_at
                 FROM books WHERE account = ?1 ORDER BY at, rowid",
            )
            .map_err(|e| self.err(e))?;
        stmt.query_map([account], entry_of)
            .and_then(|rows| rows.collect())
            .map_err(|e| self.err(e))
    }
}
