//! stablehand: one home for agent engine accounts, their logins and their
//! usage windows (README.md, docs/adr/0001): the account register and the books.
mod account;
mod books;
mod curl;
mod readers;
mod sesame;
mod store;
mod time;
mod usage;
// build.rs runs it to stamp the version; compiled here too, for its tests.
#[cfg_attr(not(test), allow(dead_code))]
mod version;

use account::{Account, Change, Engine, Field, Off, Terms};
use agent_help::Help;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use serde_json::{Value, json};
use std::process::ExitCode;
use store::Store;

agent_help::binary!("stablehand");

/// Who keeps the register: the person, or an agent for them.
const REGISTER: &str = "Keep the register";
/// Who reads the usage: any caller that starts sessions on an account.
const USAGE: &str = "Read the usage";
/// The manual itself.
const LEARN: &str = "Learn";
/// A command line clap cannot read.
const EXIT_USAGE: u8 = 2;

/// One home for agent engine accounts (Claude Code, Codex, pi, Kimi Code, agy, Grok, dsh): how each logs in, where it may run, and how much of its usage windows is left.
///
/// `stablehand read` asks an account's engine for its windows and books the reading, `stablehand limit` books a session's report that it met the account's limit, and `stablehand status` says what the books say now, each window's pace with it.
///
/// Start with `stablehand account list`. Run `stablehand <command> --help` before a command's first run.
///
/// ## Rules to know first
///
/// - Every command prints one JSON object on stdout. A refusal is `{"error": "…"}`, each fault with the line that fixes it, exit 1, and nothing written; a command line stablehand cannot read is the same with exit {const:EXIT_USAGE}. `--help`, `help` and `--version` answer text.
/// - The register and the books are one SQLite file, `$STABLEHAND_CONFIG_DIR/stablehand.db` (default `~/.config/stablehand`). It keeps no token.
#[derive(Parser)]
#[command(
    name = "stablehand",
    version = env!("STABLEHAND_VERSION"),
    disable_help_subcommand = true,
    verbatim_doc_comment
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add, list, show, change or remove an account.
    #[command(next_help_heading = REGISTER, verbatim_doc_comment)]
    Account {
        #[command(subcommand)]
        cmd: AccountCmd,
    },
    /// Switch an account off: nothing is picked on it until `stablehand on`.
    ///
    /// Keeps who (`$USER`), when (UTC) and why; switching off an account already off replaces them. Answers `{"account": {…}}`.
    #[command(next_help_heading = REGISTER, verbatim_doc_comment)]
    Off {
        name: String,
        /// Why it is off, kept with it.
        #[arg(long, verbatim_doc_comment)]
        reason: String,
    },
    /// Switch an account back on, forgetting why it was off.
    ///
    /// An account already on stays as it is. Answers `{"account": {…}}`.
    #[command(next_help_heading = REGISTER, verbatim_doc_comment)]
    On { name: String },
    /// Read an account's usage windows from its engine, and book the reading.
    ///
    /// Without a name, every account. An account is read at most once an hour: one read or tried in the last hour waits, unless `--now`. A reader's failure is booked as a failed try and said; the account's last windows are kept, marked stale. A failed read still exits 0.
    ///
    /// ## What it answers
    ///
    /// `{"reads": [{"account", "outcome", "windows", …}]}`, `outcome` one of `read`, `failed` (with `error`, the reason and its fix), `waits` (with `next_at`) or `no_reader` (its engine has no reader yet); each window as `stablehand status` shows it.
    ///
    /// ## What trips you up
    ///
    /// A claude-code account whose login is a `claude setup-token` token is refused a usage query: its read fails, and its windows come from `stablehand limit` alone.
    #[command(next_help_heading = USAGE, verbatim_doc_comment)]
    Read {
        /// The account to read; without it, every account.
        #[arg(verbatim_doc_comment)]
        name: Option<String>,
        /// Read even if it was read or tried in the last hour.
        #[arg(long, verbatim_doc_comment)]
        now: bool,
    },
    /// Report that a session met an account's limit.
    ///
    /// Books it now: the window it names is used up until `--resets-at`, or, with no `--resets-at`, until the next reading. Without `--window` the whole account is used up.
    ///
    /// ## What it answers
    ///
    /// `{"limit": {"account", "at", "window", "resets_at"}}`. It refuses, booking nothing, a `--resets-at` that is no RFC 3339 time or is past, and an empty `--window`.
    #[command(next_help_heading = USAGE, verbatim_doc_comment)]
    Limit {
        name: String,
        /// The engine's name for the window that ran out (`five_hour`, `seven_day`).
        #[arg(long, verbatim_doc_comment)]
        window: Option<String>,
        /// When the limit lifts, RFC 3339 (`2026-11-24T15:00:00Z`).
        #[arg(long, verbatim_doc_comment)]
        resets_at: Option<String>,
    },
    /// What the books say of each account now: its windows and their pace.
    ///
    /// Reads no engine (`stablehand read` does). Times are UTC RFC 3339.
    ///
    /// ## What it answers
    ///
    /// `{"accounts": [{"name", "engine", "off", "pace", "windows", "limit", "read_at", "tried_at", "failed", "read_next"}]}`. Each window is `{"name", "group" (null where the engine groups none), "used_percent" (null once it has reset since it was read), "resets_at", "source" ("read" or "limit"), "at" (when that was booked), "stale" (a later try failed, or it has reset since), "pace"}`.
    ///
    /// ## Pace
    ///
    /// A window's pace is `tight` (at its recent rate it runs out before it resets), `spare` (it resets within a quarter of its length with 20% or more left), `used_up`, or `steady` (neither, or it cannot be told). The account's is its worst window's, `used_up` while a limit holds. A reading over two hours old, or a window under half used, is never tight on its own.
    #[command(next_help_heading = USAGE, verbatim_doc_comment)]
    Status {
        /// The account; without it, every account.
        #[arg(verbatim_doc_comment)]
        name: Option<String>,
    },
    /// Show one topic of this manual, or all of it.
    ///
    /// `stablehand help all` shows the whole manual, every command in full; without a topic it shows what `stablehand --help` does.
    #[command(next_help_heading = LEARN, verbatim_doc_comment)]
    Help {
        /// The topic, or `all`.
        #[arg(verbatim_doc_comment)]
        topic: Option<String>,
    },
}

#[derive(Subcommand)]
enum AccountCmd {
    /// Add an account.
    ///
    /// Answers `{"account": {"name", "engine", "login" ("portable" or "bound"), "machine", "sessions", "expires", "home", "off"}}`, a field it lacks null. Refuses, writing nothing, a name already kept, an unknown engine (exit {const:EXIT_USAGE}), a date that is no YYYY-MM-DD day, `--expires` with `--machine` (a bound login has no token to expire) and `--home` without `--machine`.
    #[command(verbatim_doc_comment)]
    Add {
        /// The account's name, one word (claude-felix).
        #[arg(verbatim_doc_comment)]
        name: String,
        /// Its engine.
        #[arg(long, value_enum, verbatim_doc_comment)]
        engine: Engine,
        #[command(flatten)]
        terms: Terms,
    },
    /// Every account, by name.
    ///
    /// Answers `{"accounts": [{…}, …]}`, each as `stablehand account add` answers it.
    #[command(verbatim_doc_comment)]
    List,
    /// One account.
    ///
    /// Answers `{"account": {…}}`, as `stablehand account add` does.
    #[command(verbatim_doc_comment)]
    Show { name: String },
    /// Change an account's terms: only those given change.
    ///
    /// Answers `{"account": {…}}`; refuses as `stablehand account add` does, judging the account as it would be.
    #[command(verbatim_doc_comment)]
    Edit {
        name: String,
        /// Its engine.
        #[arg(long, value_enum, verbatim_doc_comment)]
        engine: Option<Engine>,
        #[command(flatten)]
        terms: Terms,
        /// Clear a term: machine (the login becomes portable), sessions (no limit), expires, home. Repeatable.
        #[arg(long, value_enum, verbatim_doc_comment)]
        unset: Vec<Field>,
    },
    /// Remove an account and its books.
    ///
    /// Answers `{"name": …, "removed": true}`, or false when there was none: run twice, the same register.
    #[command(verbatim_doc_comment)]
    Rm { name: String },
}

/// The manual: clap's command tree, and the constants it places by name.
fn manual() -> Help {
    Help::new(Cli::command()).constant("EXIT_USAGE", EXIT_USAGE)
}

fn main() -> ExitCode {
    let help = manual();
    let parsed = help.clap().map_err(|e| e.to_string()).map(|command| {
        command
            .try_get_matches()
            .and_then(|m| Cli::from_arg_matches(&m))
    });
    let cli = match parsed {
        Err(broken) => return answer(Err(broken), ExitCode::FAILURE),
        Ok(Ok(cli)) => cli,
        Ok(Err(e)) if !e.use_stderr() => e.exit(),
        Ok(Err(e)) => {
            let msg = e.render().to_string();
            return answer(Err(msg.trim().to_string()), ExitCode::from(EXIT_USAGE));
        }
    };
    if let Cmd::Help { topic } = &cli.cmd {
        let text = match topic {
            None => help.index(),
            Some(topic) => help.topic_help(topic),
        };
        return match text {
            Ok(markdown) => {
                print!("{markdown}");
                ExitCode::SUCCESS
            }
            Err(e) => answer(Err(e.to_string()), ExitCode::from(EXIT_USAGE)),
        };
    }
    answer(run(cli.cmd), ExitCode::FAILURE)
}

fn answer(result: Result<Value, String>, failure: ExitCode) -> ExitCode {
    match result {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{}", json!({ "error": e }));
            failure
        }
    }
}

fn faults(f: Vec<String>) -> String {
    f.join("; ")
}

fn shown(a: &Account) -> Value {
    json!({ "account": a.shown() })
}

/// The account `name`, or the refusal that says how to find one.
fn kept(store: &Store, name: &str) -> Result<Account, String> {
    store.get(name)?.ok_or_else(|| {
        format!(
            "no account `{name}`: `stablehand account list` names those kept; add it with `stablehand account add {name} --engine <engine>`"
        )
    })
}

fn run(cmd: Cmd) -> Result<Value, String> {
    let store = Store::open()?;
    match cmd {
        Cmd::Account { cmd } => match cmd {
            AccountCmd::Add {
                name,
                engine,
                terms,
            } => {
                let a = account::new(
                    &name,
                    engine,
                    Change {
                        terms,
                        ..Change::default()
                    },
                )
                .map_err(faults)?;
                if !store.insert(&a)? {
                    return Err(format!(
                        "account `{name}` is already kept: change it with `stablehand account edit {name} …`, or remove it first with `stablehand account rm {name}`"
                    ));
                }
                Ok(shown(&a))
            }
            AccountCmd::List => {
                let all = store.list()?;
                let shown: Vec<_> = all.iter().map(Account::shown).collect();
                Ok(json!({ "accounts": shown }))
            }
            AccountCmd::Show { name } => kept(&store, &name).map(|a| shown(&a)),
            AccountCmd::Edit {
                name,
                engine,
                terms,
                unset,
            } => {
                let a = account::apply(
                    &kept(&store, &name)?,
                    Change {
                        engine,
                        terms,
                        unset,
                    },
                )
                .map_err(faults)?;
                store.put(&a)?;
                Ok(shown(&a))
            }
            AccountCmd::Rm { name } => {
                let removed = store.remove(&name)?;
                Ok(json!({ "name": name, "removed": removed }))
            }
        },
        Cmd::Off { name, reason } => {
            let mut a = kept(&store, &name)?;
            if reason.trim().is_empty() {
                return Err("--reason is empty: say why the account is off".into());
            }
            a.off = Some(Off {
                by: std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
                at: time::utc(now()?),
                reason,
            });
            store.put(&a)?;
            Ok(shown(&a))
        }
        Cmd::On { name } => {
            let mut a = kept(&store, &name)?;
            if a.off.take().is_some() {
                store.put(&a)?;
            }
            Ok(shown(&a))
        }
        Cmd::Read { name, now: anyway } => {
            usage::read(&store, &chosen(&store, name)?, now()?, anyway)
        }
        Cmd::Limit {
            name,
            window,
            resets_at,
        } => usage::limit(&store, &kept(&store, &name)?, window, resets_at, now()?),
        Cmd::Status { name } => usage::status(&store, &chosen(&store, name)?, now()?),
        // Answered in main, before the store is opened.
        Cmd::Help { .. } => Ok(Value::Null),
    }
}

/// The clock: seconds since the Unix epoch.
fn now() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| e.to_string())
}

/// The account `name`, or every account where none is named.
fn chosen(store: &Store, name: Option<String>) -> Result<Vec<Account>, String> {
    match name {
        Some(name) => kept(store, &name).map(|a| vec![a]),
        None => store.list(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manual_is_whole() {
        Cli::command().debug_assert();
        agent_help::check(&manual()).unwrap();
        let all = manual().manual().unwrap();
        assert!(!all.contains("{const:"), "{all}");
    }
}
