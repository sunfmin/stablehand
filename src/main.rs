//! stablehand: one home for agent engine accounts, their logins and their
//! usage windows (README.md, docs/adr/0001). Today: the account register.
mod account;
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

Every command but `env` prints one JSON object on stdout. A refusal is {\"error\": \"…\"},
each fault with the line that fixes it, exit 1, and nothing written; a command
line clap cannot read is the same with exit 2. One command answers shell text:
`stablehand env <name>` prints a session's `export` lines (--json: one object),
and refuses an account no session may start on with exit 2. Start with
`stablehand account list`; run `stablehand <command> --help` before a command's
first use.
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
    /// What a session on an account starts with: its engine's variables.
    ///
    /// Portable: the engine's token variable (below), its value the token
    /// sesame keeps (`sesame token stablehand-<name> --json`). Bound: the
    /// engine's home variable (below) set to the account's --home, on its
    /// machine only ($STABLEHAND_MACHINE, else `hostname -s`). Prints one
    /// `export NAME='value'` line each, the value one shell word, for a shell
    /// to read (`. <file>`, or `eval`); with --json, {"name", "login",
    /// "env": {NAME: value}}. Written to stdout alone: no file, and no child
    /// is given a value on its command line. Refuses, exit 2, with the fix,
    /// an account off, not logged in, bound to another machine or with no
    /// home, and a portable one of an engine with no token variable; exit 1
    /// is no such account, or sesame that could not be asked.
    #[command(after_long_help = engines_help())]
    Env {
        name: String,
        /// Answer one JSON object instead of `export` lines.
        #[arg(long)]
        json: bool,
    },
    #[command(flatten)]
    Register(Register),
}

/// The commands that read and change the register.
#[derive(Subcommand)]
enum Register {
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
                "  {}: bound `{}`, {}; token {token}, in {}",
                e.name(),
                l.bound.join(" "),
                l.home_var,
                l.token_var.unwrap_or("none: bound only")
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
    match cli.cmd {
        Cmd::Env { name, json } => match env(&name, json) {
            Ok(Err(refusal)) => answer(Err(refusal), ExitCode::from(2)),
            Ok(text) => answer(text, ExitCode::FAILURE),
            Err(e) => answer(Err(e), ExitCode::FAILURE),
        },
        Cmd::Register(cmd) => answer(run(cmd).map(|v| format!("{v}\n")), ExitCode::FAILURE),
    }
}

/// Prints the answer, its text as it is, or the fault as {"error": …},
/// exiting `failure`.
fn answer(result: Result<String, String>, failure: ExitCode) -> ExitCode {
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{}", json!({ "error": e }));
            failure
        }
    }
}

/// `env`'s answer: the account's variable as an `export NAME='value'` line,
/// the value one shell word whatever it holds (the text of factorium's
/// secrets file, factorium_harness::secrets_file), or with `json` as
/// {"name", "login", "env": {NAME: value}}; `Ok(Err)` a refusal (exit 2).
fn env(name: &str, json: bool) -> Result<Result<String, String>, String> {
    let a = kept(&Store::open()?, name)?;
    Ok(login::env(&a)?.map(|(var, value)| {
        if json {
            let env = json!({ var: value });
            format!(
                "{}\n",
                json!({ "name": a.name, "login": a.login(), "env": env })
            )
        } else {
            format!("export {var}='{}'\n", value.replace('\'', "'\\''"))
        }
    }))
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

fn run(cmd: Register) -> Result<Value, String> {
    let store = Store::open()?;
    match cmd {
        Register::Account { cmd } => match cmd {
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
            AccountCmd::Rm { name } => {
                let removed = store.remove(&name)?;
                Ok(json!({ "name": name, "removed": removed }))
            }
        },
        Register::Off { name, reason } => {
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
        Register::On { name } => {
            let mut a = kept(&store, &name)?;
            if a.off.take().is_some() {
                store.put(&a)?;
            }
            Ok(shown(&a))
        }
    }
}
