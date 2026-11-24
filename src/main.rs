//! stablehand: one home for agent engine accounts, their logins and their
//! usage windows (README.md, docs/adr/0001). Today: the account register.
mod account;
mod store;
// build.rs runs it to stamp the version; compiled here too, for its tests.
#[cfg_attr(not(test), allow(dead_code))]
mod version;

use account::{Account, Change, Engine, Field, Off, Terms};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::process::ExitCode;
use store::Store;

const FOR_AI: &str = "\
stablehand keeps each agent engine account (Claude Code, Codex, pi, Kimi Code,
agy, Grok, dsh): how it logs in, where it may run, and, as later versions add
it, how much of its usage windows is left.

Every command prints one JSON object on stdout. A refusal is {\"error\": \"…\"},
each fault with the line that fixes it, exit 1, and nothing written; a command
line clap cannot read is the same with exit 2. Start with `stablehand account
list`; run `stablehand <command> --help` before a command's first use.
The register is one SQLite file, $STABLEHAND_CONFIG_DIR/stablehand.db
(default ~/.config/stablehand). It keeps no token.";

#[derive(Parser)]
#[command(name = "stablehand", version = env!("STABLEHAND_VERSION"), before_help = FOR_AI)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add, list, show, change or remove an account.
    Account {
        #[command(subcommand)]
        cmd: AccountCmd,
    },
    /// Switch an account off: nothing is picked on it until `stablehand on`.
    ///
    /// Keeps who (`$USER`), when (UTC) and why; switching off an account
    /// already off replaces them. Answers {"account": {…}}.
    Off {
        name: String,
        /// Why it is off, kept with it.
        #[arg(long)]
        reason: String,
    },
    /// Switch an account back on, forgetting why it was off.
    ///
    /// An account already on stays as it is. Answers {"account": {…}}.
    On { name: String },
}

#[derive(Subcommand)]
enum AccountCmd {
    /// Add an account.
    ///
    /// Answers {"account": {name, engine, login ("portable" or
    /// "bound"), machine, sessions, expires, home, off}}, a field it lacks
    /// null. Refuses, writing nothing, a name already kept, an unknown engine
    /// (exit 2),
    /// a date that is no YYYY-MM-DD day, --expires with --machine (a bound
    /// login has no token to expire) and --home without --machine.
    Add {
        /// The account's name, one word (claude-felix).
        name: String,
        /// Its engine.
        #[arg(long, value_enum)]
        engine: Engine,
        #[command(flatten)]
        terms: Terms,
    },
    /// Every account, by name.
    ///
    /// Answers {"accounts": [{…}, …]}, each as `add` answers it.
    List,
    /// One account.
    ///
    /// Answers {"account": {…}}, as `add` does.
    Show { name: String },
    /// Change an account's terms: only those given change.
    ///
    /// Answers {"account": {…}}; refuses as `add` does, judging the account as it would be.
    Edit {
        name: String,
        /// Its engine.
        #[arg(long, value_enum)]
        engine: Option<Engine>,
        #[command(flatten)]
        terms: Terms,
        /// Clear a term: machine (the login becomes portable), sessions (no limit), expires, home. Repeatable.
        #[arg(long, value_enum)]
        unset: Vec<Field>,
    },
    /// Remove an account.
    ///
    /// Answers {"name": …, "removed": true}, or false when
    /// there was none: run twice, the same register.
    Rm { name: String },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if !e.use_stderr() => e.exit(),
        Err(e) => {
            let msg = e.render().to_string();
            return answer(Err(msg.trim().to_string()), ExitCode::from(2));
        }
    };
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
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs();
            a.off = Some(Off {
                by: std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
                at: account::utc(now),
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
    }
}
