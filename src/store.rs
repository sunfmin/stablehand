//! The register's and the books' one store: a SQLite database, `<config dir>/stablehand.db`,
//! the config dir `$STABLEHAND_CONFIG_DIR` or `~/.config/stablehand`, guarded
//! the directory 0700, the file 0600. It holds no secret: of a portable
//! login it keeps the command that prints its token, never the token.
//!
//! Only a released stablehand keeps its store in `~/.config/stablehand`
//! unasked: a development build's steps may not be the released ones
//! (stablehand#27), so it opens only where `$STABLEHAND_CONFIG_DIR` points.
use crate::account::{Account, Engine, Off};
use crate::books::Entry;
use crate::order::Rank;
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The schema, one step per `PRAGMA user_version`: append a step, never edit
/// one. Each step run is kept in the store by its hash (`schema_steps`), so
/// a store another list of steps made is refused, never misread.
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
    "
    -- The books: one row per entry, what it says as JSON (books::What).
    CREATE TABLE books (
        account TEXT NOT NULL,
        at INTEGER NOT NULL,
        what TEXT NOT NULL
    );
    CREATE INDEX books_by_account ON books (account, at);
",
    // A bound login is its home's login file, a portable one its token
    // command's answer (docs/adr/0001): the register records no login.
    "
    ALTER TABLE accounts DROP COLUMN logged_in_at;
    ALTER TABLE accounts ADD COLUMN token_command TEXT;
",
    // The hash of the identity `account scan` read in the account's login.
    "ALTER TABLE accounts ADD COLUMN identity TEXT;",
    // The order, one row: its ranks (order::Rank), highest first, as JSON.
    "CREATE TABLE engine_order (id INTEGER PRIMARY KEY CHECK (id = 1), ranks TEXT NOT NULL);",
    // No sessions-at-once term: the pace counts the sessions running (stablehand#26).
    "ALTER TABLE accounts DROP COLUMN sessions;",
    // The sessions `stablehand run` started and waits for, by the pid of
    // the engine it started: a row outlives a run killed before it ended,
    // and is dropped once that pid is found dead.
    "CREATE TABLE sessions (pid INTEGER PRIMARY KEY, account TEXT NOT NULL, started INTEGER NOT NULL);",
    // Each engine's command `run` starts it with, one row: `[[engine,
    // [words…]]]` as JSON (docs/adr/0003); none kept on a fresh store.
    "CREATE TABLE engine_commands (id INTEGER PRIMARY KEY CHECK (id = 1), commands TEXT NOT NULL);",
    // A bound claude-code login's organization, its hash and its type,
    // which a portable token's identity is compared with (stablehand#31).
    "
    ALTER TABLE accounts ADD COLUMN organization TEXT;
    ALTER TABLE accounts ADD COLUMN organization_type TEXT;
",
];

/// How long a process waits for another's write.
const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

const COLUMNS: &str = "name, engine, machine, expires, home, off_by, off_at, off_reason, token_command, identity, organization, organization_type";

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

/// `$STABLEHAND_CONFIG_DIR`, else `~/.config/stablehand` for a released
/// build (one `packaging/release.sh` stamped); a development build refuses it.
fn config_dir(
    set: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    release: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(d) = set {
        return Ok(PathBuf::from(d));
    }
    if release.is_none() {
        return Err(format!(
            "stablehand {} is a development build: its schema steps may not be the released ones, so it never opens ~/.config/stablehand unasked. Set STABLEHAND_CONFIG_DIR to a directory of its own (a copy of the store, if it needs the accounts), or run the installed stablehand",
            env!("STABLEHAND_VERSION")
        ));
    }
    Ok(PathBuf::from(home.unwrap_or_default())
        .join(".config")
        .join("stablehand"))
}

/// A schema step as the store keeps it: its SHA-256, hex.
fn step_hash(step: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(step);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A step as a refusal names it: its first line of SQL.
fn step_line(step: &str) -> &str {
    step.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("--"))
        .unwrap_or("")
}

/// Brings the store of `conn` to the last of `steps`, after checking each
/// step it ran is the same step of `steps`, all in one transaction: a
/// refusal or a failed step leaves the store as it was. A store from before
/// the steps were kept (none recorded) is taken to have run these.
fn migrate(conn: &Connection, steps: &[&str]) -> Result<(), String> {
    let e = |e: rusqlite::Error| e.to_string();
    // Immediate: two processes opening a new store wait for each other
    // (BUSY_TIMEOUT) rather than both reading its version and one failing.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(e)?;
    let at: i64 = tx
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(e)?;
    let at = at as usize;
    if at > steps.len() {
        return Err(format!(
            "the store is at schema step {at}, past this stablehand's last, {}: a later stablehand made it. Run that one (brew upgrade stablehand)",
            steps.len()
        ));
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_steps (step INTEGER PRIMARY KEY, hash TEXT NOT NULL);",
    )
    .map_err(e)?;
    let mut first = 0;
    {
        let mut stmt = tx
            .prepare("SELECT step, hash FROM schema_steps ORDER BY step")
            .map_err(e)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)? as usize, r.get::<_, String>(1)?))
            })
            .map_err(e)?;
        for row in rows {
            let (n, hash) = row.map_err(e)?;
            first = at;
            let ours = steps.get(n - 1);
            if n > at || ours.map(|s| step_hash(s)) != Some(hash) {
                let theirs = ours.map_or("none", |s| step_line(s));
                return Err(format!(
                    "schema step {n} of the store is not this stablehand's step {n} (`{theirs}`): another build's steps migrated it, and these would misread it. Restore the store's backup from before that build ran, or repair it by hand to what these steps make"
                ));
            }
        }
    }
    for (i, step) in steps.iter().enumerate().skip(first) {
        let n = i + 1;
        let run = if i < at { "" } else { step };
        tx.execute_batch(&format!(
            "{run} INSERT INTO schema_steps (step, hash) VALUES ({n}, '{}'); PRAGMA user_version = {};",
            step_hash(step),
            n.max(at)
        ))
        .map_err(|err| format!("schema step {n} (`{}`): {err}", step_line(step)))?;
    }
    tx.commit().map_err(e)
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
    let off = match (row.get(5)?, row.get(6)?, row.get(7)?) {
        (Some(by), Some(at), Some(reason)) => Some(Off { by, at, reason }),
        _ => None,
    };
    Ok(Account {
        name: row.get(0)?,
        engine: engine_of(row)?,
        machine: row.get(2)?,
        expires: row.get(3)?,
        home: row.get(4)?,
        off,
        token_command: row.get(8)?,
        identity: row.get(9)?,
        organization: row.get(10)?,
        organization_type: row.get(11)?,
    })
}

impl Store {
    pub fn open() -> Result<Store, String> {
        let dir = config_dir(
            std::env::var_os("STABLEHAND_CONFIG_DIR"),
            std::env::var_os("HOME"),
            option_env!("STABLEHAND_RELEASE"),
        )?;
        let path = dir.join("stablehand.db");
        prepare(&path)?;
        let conn = Connection::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let store = Store { conn, path };
        // No WAL: the rollback journal SQLite makes takes the database's 0600.
        store
            .conn
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(|e| store.err(e))?;
        migrate(&store.conn, MIGRATIONS).map_err(|e| format!("{}: {e}", store.path.display()))?;
        Ok(store)
    }

    fn err(&self, e: rusqlite::Error) -> String {
        format!("{}: {e}", self.path.display())
    }

    /// The file a read of `account` the status line started holds locked
    /// while it runs, beside the register.
    pub fn reading_lock(&self, account: &str) -> PathBuf {
        self.path.with_file_name(format!("reading-{account}.lock"))
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
                    "{verb} accounts ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
                ),
                params![
                    a.name,
                    a.engine.name(),
                    a.machine,
                    a.expires,
                    a.home,
                    off.map(|o| &o.by),
                    off.map(|o| &o.at),
                    off.map(|o| &o.reason),
                    a.token_command,
                    a.identity,
                    a.organization,
                    a.organization_type,
                ],
            )
            .map(|n| n == 1)
            .map_err(|e| self.err(e))
    }

    /// Keeps on the account of `read`'s name the hashes `account scan` read
    /// of its login, each only where it has none: what it had stays.
    pub fn fill_hashes(&self, read: &Account) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE accounts SET identity = COALESCE(identity, ?2),
                    organization = COALESCE(organization, ?3),
                    organization_type = COALESCE(organization_type, ?4)
                 WHERE name = ?1",
                params![
                    read.name,
                    read.identity,
                    read.organization,
                    read.organization_type
                ],
            )
            .map(|_| ())
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

impl Store {
    /// The order, highest first; empty when none is set.
    pub fn order(&self) -> Result<Vec<Rank>, String> {
        self.one("SELECT ranks FROM engine_order", "the order")
    }

    /// Replaces the order with `order`.
    pub fn set_order(&self, order: &[Rank]) -> Result<(), String> {
        self.set_one(
            "REPLACE INTO engine_order (id, ranks) VALUES (1, ?1)",
            order,
        )
    }

    /// Each engine's kept command, its words, by engine name; empty when none is kept.
    pub fn commands(&self) -> Result<Vec<(Engine, Vec<String>)>, String> {
        self.one("SELECT commands FROM engine_commands", "the commands")
    }

    /// Replaces the kept commands with `commands`.
    pub fn set_commands(&self, commands: &[(Engine, Vec<String>)]) -> Result<(), String> {
        let sql = "REPLACE INTO engine_commands (id, commands) VALUES (1, ?1)";
        self.set_one(sql, commands)
    }

    /// What the one-row table `sql` reads holds, as JSON; its default when empty.
    fn one<T: serde::de::DeserializeOwned + Default>(
        &self,
        sql: &str,
        what: &str,
    ) -> Result<T, String> {
        let text: Option<String> = self
            .conn
            .query_row(sql, [], |r| r.get(0))
            .optional()
            .map_err(|e| self.err(e))?;
        text.map_or(Ok(T::default()), |t| {
            serde_json::from_str(&t).map_err(|e| format!("{}: {what}: {e}", self.path.display()))
        })
    }

    /// Replaces what the one-row table `sql` writes with `value`, as JSON.
    fn set_one<T: serde::Serialize + ?Sized>(&self, sql: &str, value: &T) -> Result<(), String> {
        let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
        self.conn
            .execute(sql, [text])
            .map(|_| ())
            .map_err(|e| self.err(e))
    }
}

fn entry_of(row: &Row) -> rusqlite::Result<Entry> {
    let text: String = row.get(1)?;
    let what = serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, e.into())
    })?;
    Ok(Entry {
        at: row.get::<_, i64>(0)? as u64,
        what,
    })
}

impl Store {
    /// Books `entry` in `account`'s books.
    pub fn book(&self, account: &str, entry: &Entry) -> Result<(), String> {
        let what = serde_json::to_string(&entry.what).map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "INSERT INTO books (account, at, what) VALUES (?1, ?2, ?3)",
                params![account, entry.at as i64, what],
            )
            .map(|_| ())
            .map_err(|e| self.err(e))
    }

    /// `account`'s books, oldest first.
    pub fn entries(&self, account: &str) -> Result<Vec<Entry>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT at, what FROM books WHERE account = ?1 ORDER BY at, rowid")
            .map_err(|e| self.err(e))?;
        stmt.query_map([account], entry_of)
            .and_then(|rows| rows.collect())
            .map_err(|e| self.err(e))
    }
}

impl Store {
    /// Keeps the session `run` started on `account`, its engine's `pid`.
    pub fn start_session(&self, account: &str, pid: u32, at: u64) -> Result<(), String> {
        self.conn
            .execute(
                "REPLACE INTO sessions (pid, account, started) VALUES (?1, ?2, ?3)",
                params![pid, account, at as i64],
            )
            .map(|_| ())
            .map_err(|e| self.err(e))
    }

    /// Drops the session of `pid`.
    pub fn end_session(&self, pid: u32) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM sessions WHERE pid = ?1", [pid])
            .map(|_| ())
            .map_err(|e| self.err(e))
    }

    /// Each session kept, `(pid, account)`.
    pub fn sessions(&self) -> Result<Vec<(u32, String)>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT pid, account FROM sessions ORDER BY pid")
            .map_err(|e| self.err(e))?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .and_then(|rows| rows.collect())
            .map_err(|e| self.err(e))
    }
}

#[cfg(test)]
mod tests {
    use super::{Connection, MIGRATIONS, config_dir, migrate};
    use std::path::PathBuf;

    fn version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn a_store_another_list_of_steps_migrated_is_refused_by_the_step() {
        // stablehand#27: a branch's list put the sessions table at step 7,
        // where the released list drops accounts.sessions.
        let conn = Connection::open_in_memory().unwrap();
        let branch = [&MIGRATIONS[..6], &[MIGRATIONS[7]]].concat();
        migrate(&conn, &branch).unwrap();
        let fault = migrate(&conn, MIGRATIONS).unwrap_err();
        assert!(fault.contains("schema step 7 of the store"), "{fault}");
        assert!(
            fault.contains("`ALTER TABLE accounts DROP COLUMN sessions;`"),
            "{fault}"
        );
        assert!(!fault.contains("already exists"), "{fault}");
        // Nothing of the released steps ran.
        assert_eq!(version(&conn), 7);
    }

    #[test]
    fn a_store_from_before_the_steps_were_kept_records_none_when_a_step_fails() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn, &MIGRATIONS[..2]).unwrap();
        conn.execute_batch("DROP TABLE schema_steps; CREATE TABLE books (x);")
            .unwrap();
        let fault = migrate(&conn, MIGRATIONS).unwrap_err();
        assert!(
            fault.starts_with("schema step 3 (`CREATE TABLE books (`)"),
            "{fault}"
        );
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'schema_steps'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((version(&conn), tables), (2, 0));
    }

    #[test]
    fn steps_run_once_each_kept_and_a_store_from_before_they_were_kept_is_taken_as_is() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn, &MIGRATIONS[..3]).unwrap();
        // A store made before schema_steps: its steps are taken as these.
        conn.execute_batch("DROP TABLE schema_steps;").unwrap();
        migrate(&conn, MIGRATIONS).unwrap();
        migrate(&conn, MIGRATIONS).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        let kept: i64 = conn
            .query_row("SELECT count(*) FROM schema_steps", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, MIGRATIONS.len() as i64);
        // A later list of steps: this one refuses its store, reading nothing.
        let later = [MIGRATIONS, &["CREATE TABLE later (x);"]].concat();
        migrate(&conn, &later).unwrap();
        let fault = migrate(&conn, MIGRATIONS).unwrap_err();
        assert!(fault.contains("past this stablehand's last"), "{fault}");
    }

    #[test]
    fn a_step_that_fails_is_named_and_leaves_the_store_as_it_was() {
        let conn = Connection::open_in_memory().unwrap();
        let steps = ["CREATE TABLE a (x);", "CREATE TABLE a (x);"];
        let fault = migrate(&conn, &steps).unwrap_err();
        assert!(
            fault.starts_with("schema step 2 (`CREATE TABLE a (x);`)"),
            "{fault}"
        );
        assert_eq!(version(&conn), 0);
        // The connection is usable after: no transaction left open.
        migrate(&conn, &steps[..1]).unwrap();
    }

    #[test]
    fn only_a_released_build_opens_the_host_store_unasked() {
        let home = Some("/h".into());
        let dir = config_dir(None, home.clone(), Some("v0.2.0")).unwrap();
        assert_eq!(dir, PathBuf::from("/h/.config/stablehand"));
        let fault = config_dir(None, home.clone(), None).unwrap_err();
        assert!(fault.contains("development build"), "{fault}");
        assert!(fault.contains("STABLEHAND_CONFIG_DIR"), "{fault}");
        let set = config_dir(Some("/s".into()), home, None).unwrap();
        assert_eq!(set, PathBuf::from("/s"));
    }
}
