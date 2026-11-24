//! stablehand: one home for agent engine accounts, their logins and their
//! usage windows (README.md, docs/adr/0001). Today: the account register.
mod account;
mod import;
mod login;
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
    /// One account, and whether it is logged in.
    ///
    /// Answers {"account": {…}, "logged_in": true|false, "days_left": n|null,
    /// "warning": "…"|null}: a bound login is logged in as `account login`
    /// recorded it (`logged_in_at`), a portable one when sesame holds its
    /// token (`sesame token stablehand-<name>`); `days_left` counts to
    /// --expires, and `warning` says from 30 days ahead to make a new token.
    Show { name: String },
    /// Log an account in by its engine's own login.
    ///
    /// Portable: runs the engine's token command (below; with --token,
    /// none), then sesame asks for the token with hidden input, or reads it
    /// from stdin when stdin is no terminal, and keeps it as the provider
    /// `stablehand-<name>` (made with `sesame provider add` if missing). The
    /// token is on no command line, in no log and not in the register. Bound:
    /// on the account's machine ($STABLEHAND_MACHINE, else `hostname -s`),
    /// runs the engine's login (below) with the variable it reads for its home
    /// set to the account's --home, and records when. Answers as `show` does.
    /// Refuses, writing nothing, a bound login on another machine or with no
    /// home, --token or --expires on a bound login, and an engine with no
    /// token command without --token.
    #[command(after_long_help = engines_help())]
    Login {
        name: String,
        /// Paste the token instead of running the engine's token command.
        #[arg(long)]
        token: bool,
        /// When the new token expires, YYYY-MM-DD, kept as the account's --expires.
        #[arg(long)]
        expires: Option<String>,
    },
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
    /// Move this host's `agent-account-*` entries of mytokens into stablehand.
    ///
    /// Each entry `agent-account-<name>` (`mytokens list`) becomes the account
    /// <name>: its engine from its HARNESS, bound to its MACHINE if it has one;
    /// its CLAUDE_CODE_OAUTH_TOKEN, if it has one, goes from `mytokens get`
    /// straight into sesame, as `account login` keeps a token. An account
    /// already kept is left as it is. mytokens is only read: nothing in it is
    /// changed or removed. Answers {"dry_run": …, "accounts": [{"from":
    /// "agent-account-<name>/<label>", "result": "added"|"would add"|"already
    /// kept", "token": true|false, "account": {…}}, …]}, no token shown.
    /// Refuses, importing nothing, an entry whose HARNESS is no engine, one
    /// with both a token and a MACHINE, and a field mytokens does not answer;
    /// a token sesame refuses stops it there, and running it again imports the rest.
    Import {
        /// Read the accounts from mytokens (the one source today).
        #[arg(long, required = true)]
        from_mytokens: bool,
        /// Say what it would import, reading no token and writing nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove an account.
    ///
    /// Answers {"name": …, "removed": true}, or false when
    /// there was none: run twice, the same register.
    Rm { name: String },
}

/// Each engine's login, as `account login --help` lists it.
fn engines_help() -> String {
    let lines = <Engine as clap::ValueEnum>::value_variants()
        .iter()
        .map(|e| {
            let l = e.login();
            let token = l
                .token
                .map_or("none: --token".into(), |t| format!("`{}`", t.join(" ")));
            format!(
                "  {}: bound `{}`, {}; token {token}",
                e.name(),
                l.bound.join(" "),
                l.home_var
            )
        });
    format!("Engines:\n{}", lines.collect::<Vec<_>>().join("\n"))
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

fn now() -> Result<u64, String> {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map(|d| d.as_secs()).map_err(|e| e.to_string())
}

/// `a` as `show` answers it: with whether it is logged in.
fn with_status(a: &Account) -> Result<Value, String> {
    let today = (now()? / 86_400) as i64;
    let mut v = serde_json::to_value(account::status(a, login::logged_in(a)?, today))
        .map_err(|e| e.to_string())?;
    v["account"] = json!(a.shown());
    Ok(v)
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
            AccountCmd::Show { name } => with_status(&kept(&store, &name)?),
            AccountCmd::Login {
                name,
                token,
                expires,
            } => {
                let change = Change {
                    terms: Terms {
                        expires,
                        ..Terms::default()
                    },
                    ..Change::default()
                };
                let a = account::apply(&kept(&store, &name)?, change).map_err(faults)?;
                let a = login::login(&a, token, account::utc(now()?))?;
                store.put(&a)?;
                with_status(&a)
            }
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
            AccountCmd::Import { dry_run, .. } => import::import(&store, dry_run),
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
                at: account::utc(now()?),
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
