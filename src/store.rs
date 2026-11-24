//! The register's one store: a SQLite database, `<config dir>/stablehand.db`,
//! the config dir `$STABLEHAND_CONFIG_DIR` or `~/.config/stablehand`, guarded
//! as sesame keeps its own: the directory 0700, the file 0600. It holds no
//! secret: a portable login's token is sesame's.
use crate::account::{Account, Engine, Off};
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
    "ALTER TABLE accounts ADD COLUMN logged_in_at TEXT;",
];

/// How long a process waits for another's write.
const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

const COLUMNS: &str =
    "name, engine, machine, sessions, expires, home, off_by, off_at, off_reason, logged_in_at";

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
        logged_in_at: row.get(9)?,
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
                &format!(
                    "{verb} accounts ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
                ),
                params![
                    a.name,
                    a.engine.name(),
                    a.machine,
                    a.sessions,
                    a.expires,
                    a.home,
                    off.map(|o| &o.by),
                    off.map(|o| &o.at),
                    off.map(|o| &o.reason),
                    a.logged_in_at,
                ],
            )
            .map(|n| n == 1)
            .map_err(|e| self.err(e))
    }

    /// Removes the account `name`; false when there was none.
    pub fn remove(&self, name: &str) -> Result<bool, String> {
        self.conn
            .execute("DELETE FROM accounts WHERE name = ?1", [name])
            .map(|n| n == 1)
            .map_err(|e| self.err(e))
    }
}
