//! stablehand: one home for agent engine accounts, their logins and their
//! usage windows (README.md, docs/adr/0001): the account register and the books.
mod account;
mod books;
mod curl;
mod import;
mod login;
mod pick;
mod readers;
mod refusal;
mod store;
mod time;
mod usage;
// build.rs runs it to stamp the version; compiled here too, for its tests.
#[cfg_attr(not(test), allow(dead_code))]
mod version;

use account::{Account, Change, Engine, Field, Off, Terms};
use agent_help::Help;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use refusal::{Code, Refused};
use serde_json::{Value, json};
use std::process::ExitCode;
use store::Store;

agent_help::binary!("stablehand");

/// Who keeps the register: the person, or an agent for them.
const REGISTER: &str = "Keep the register";
/// Who starts sessions on the accounts: factorium, Guild, a person.
const SESSIONS: &str = "Start sessions";
/// The manual itself.
const LEARN: &str = "Learn";

/// One home for agent engine accounts (Claude Code, Codex, pi, Kimi Code, agy, Grok, dsh): how each logs in, where it may run, and how much of its usage windows is left.
///
/// `stablehand env <name>` gives a session on an account its engine's variables; `stablehand read` asks an account's engine for its windows and books the reading, `stablehand limit` books a session's report that it met the account's limit, `stablehand status` says what the books say now, each window's pace with it, and `stablehand pick --engine <engine>` which account one more session may start on.
///
/// Start with `stablehand account list`. Run `stablehand <command> --help` before a command's first run.
///
/// ## Rules to know first
///
/// - Every command but `env` prints one JSON object on stdout. A refusal is `{"error": "…", "code": "…"}`, each fault with the line that fixes it, exit 1, and nothing written; a command line stablehand cannot read is the same with exit {const:EXIT_USAGE}. `--help`, `help` and `--version` answer text.
/// - `stablehand env <name>` answers shell text, a session's `export` lines (`--json`: one object), and refuses an account no session may start on with exit {const:EXIT_USAGE}.
/// - The register and the books are one SQLite file, `$STABLEHAND_CONFIG_DIR/stablehand.db` (default `~/.config/stablehand`). It keeps no token: a portable login's token is sesame's.
///
/// ## Refusals
///
/// A refusal's `code` is one of these:
///
/// {table:refusal}
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
    /// What a session on an account starts with: its engine's variables.
    ///
    /// Portable: the engine's token variable (below), its value the token sesame keeps (`sesame token stablehand-<name> --json`). Bound: the engine's home variable (below) set to the account's `--home`, on its machine only (`$STABLEHAND_MACHINE`, else `hostname -s`).
    ///
    /// ## What it answers
    ///
    /// One `export NAME='value'` line each, the value one shell word, for a shell to read (`. <file>`, or `eval`); with `--json`, `{"name", "login", "env": {NAME: value}}`. Written to stdout alone: no file, and no child is given a value on its command line.
    ///
    /// ## What it refuses
    ///
    /// Exit {const:EXIT_USAGE}, with the fix: an account off, not logged in, bound to another machine or with no home, and a portable one of an engine with no token variable. Exit 1: no such account, or sesame that could not be asked.
    ///
    /// ## Engines
    ///
    /// {const:ENGINES}
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
    Env {
        name: String,
        /// Answer one JSON object instead of `export` lines.
        #[arg(long, verbatim_doc_comment)]
        json: bool,
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
    #[command(flatten)]
    Register(Register),
}

/// The commands that read and change the register and the books.
#[derive(Subcommand)]
enum Register {
    /// Add, list, show, log in, change, import or remove an account.
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
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
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
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
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
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
    Status {
        /// The account; without it, every account.
        #[arg(verbatim_doc_comment)]
        name: Option<String>,
    },
    /// Which account of an engine one more session may start on now.
    ///
    /// Reads no engine: it judges the books as `stablehand status` shows them (run `stablehand read` first for fresh windows). The candidates are the engine's accounts usable on this machine (portable, or bound to it), logged in, and not off.
    ///
    /// ## The rules
    ///
    /// Guild's ledger's, carried over. None is picked on an account
    ///
    /// - a caller's limit holds on (`stablehand limit`), until it lifts;
    /// - any of whose windows is `--stop-percent` used, until that window resets: the rest is the margin of the sessions already running;
    /// - running its sessions at once (`account add --sessions`), counted by `--running`;
    /// - one of whose windows, at its present rate shared by the sessions running and one more, runs out before it resets. A reading over two hours old, or a window under half used, runs on at no rate.
    ///
    /// Of the rest, the one with the most left of its most used window; one with no reading in force, or only stale ones, after every one with one; of equals, the first by name.
    ///
    /// ## What it answers
    ///
    /// `{"account": "<name>"}`, or `{"none": true, "until": <the earliest time any lifts, or null>, "why": [{"account", "reason", "why", "until"}]}`, `reason` one of `off`, `elsewhere`, `not_logged_in`, `limit`, `stopped`, `full`, `runs_out`. An engine with no account here answers none with an empty `why`. It refuses a `--running` name that is no account kept.
    ///
    /// ## What trips you up
    ///
    /// `--running` is the caller's count, on this machine or wherever it knows: stablehand counts no session itself. An account left out of it has none running.
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
    Pick {
        /// The engine the session runs in.
        #[arg(long, value_enum, verbatim_doc_comment)]
        engine: Engine,
        /// The sessions now running per account, `<account>=<n>`, comma-separated or repeated.
        #[arg(long, value_delimiter = ',', value_parser = running, verbatim_doc_comment)]
        running: Vec<(String, u32)>,
        /// The machine the session starts on; without it, this one (`$STABLEHAND_MACHINE`, else `hostname -s`).
        #[arg(long, verbatim_doc_comment)]
        machine: Option<String>,
        /// How much of any window, in percent, takes an account out of the pick until it resets.
        #[arg(long, default_value_t = pick::STOP_PERCENT, value_parser = clap::value_parser!(u8).range(1..=100), verbatim_doc_comment)]
        stop_percent: u8,
    },
}

/// One `--running` count, `<account>=<n>`.
fn running(s: &str) -> Result<(String, u32), String> {
    let bad = || format!("`{s}` is no count: give it as <account>=<n>, claude-felix=2");
    let (name, n) = s.split_once('=').ok_or_else(bad)?;
    let n = n.trim().parse().map_err(|_| bad())?;
    let name = name.trim();
    if name.is_empty() {
        return Err(bad());
    }
    Ok((name.to_string(), n))
}

#[derive(Subcommand)]
enum AccountCmd {
    /// Add an account.
    ///
    /// Answers `{"account": {"name", "engine", "login" ("portable" or "bound"), "machine", "sessions", "expires", "home", "off", "logged_in_at"}}`, a field it lacks null. Refuses, writing nothing, a name already kept, an unknown engine (exit {const:EXIT_USAGE}), a date that is no YYYY-MM-DD day, `--expires` with `--machine` (a bound login has no token to expire) and `--home` without `--machine`.
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
    /// One account, and whether it is logged in.
    ///
    /// Answers `{"account": {…}, "logged_in": true|false, "days_left": n|null, "warning": "…"|null}`: a bound login is logged in as `stablehand account login` recorded it (`logged_in_at`), a portable one when sesame holds its token (`sesame token stablehand-<name>`); `days_left` counts to `--expires`, and `warning` says from 30 days ahead to make a new token.
    #[command(verbatim_doc_comment)]
    Show { name: String },
    /// Log an account in by its engine's own login.
    ///
    /// Portable: runs the engine's token command (below; with `--token`, none), then sesame asks for the token with hidden input, or reads it from stdin when stdin is no terminal, and keeps it as the provider `stablehand-<name>` (made with `sesame provider add` if missing). The token is on no command line, in no log and not in the register. Bound: on the account's machine (`$STABLEHAND_MACHINE`, else `hostname -s`), runs the engine's login (below) with the variable it reads for its home set to the account's `--home`, and records when. Answers as `stablehand account show` does.
    ///
    /// Refuses, writing nothing, a bound login on another machine or with no home, `--token` or `--expires` on a bound login, and an engine with no token command without `--token`.
    ///
    /// ## Engines
    ///
    /// {const:ENGINES}
    #[command(verbatim_doc_comment)]
    Login {
        name: String,
        /// Paste the token instead of running the engine's token command.
        #[arg(long, verbatim_doc_comment)]
        token: bool,
        /// When the new token expires, YYYY-MM-DD, kept as the account's --expires.
        #[arg(long, verbatim_doc_comment)]
        expires: Option<String>,
    },
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
    /// Move this host's `agent-account-*` entries of mytokens into stablehand.
    ///
    /// Each entry `agent-account-<name>` (`mytokens list`) becomes the account `<name>`: its engine from its HARNESS, bound to its MACHINE if it has one; its CLAUDE_CODE_OAUTH_TOKEN, if it has one, goes from `mytokens get` straight into sesame, as `stablehand account login` keeps a token. An account already kept is left as it is. mytokens is only read: nothing in it is changed or removed.
    ///
    /// Answers `{"dry_run": …, "accounts": [{"from": "agent-account-<name>/<label>", "result": "added"|"would add"|"already kept", "token": true|false, "account": {…}}, …]}`, no token shown. Refuses, importing nothing, an entry whose HARNESS is no engine, one with both a token and a MACHINE, and a field mytokens does not answer; a token sesame refuses stops it there, and running it again imports the rest.
    #[command(verbatim_doc_comment)]
    Import {
        /// Read the accounts from mytokens (the one source today).
        #[arg(long, required = true, verbatim_doc_comment)]
        from_mytokens: bool,
        /// Say what it would import, reading no token and writing nothing.
        #[arg(long, verbatim_doc_comment)]
        dry_run: bool,
    },
    /// Remove an account and its books.
    ///
    /// Answers `{"name": …, "removed": true}`, or false when there was none: run twice, the same register.
    #[command(verbatim_doc_comment)]
    Rm { name: String },
}

/// Each engine's login, as `stablehand env --help` and `stablehand account login --help` list it.
fn engines_help() -> String {
    let lines = <Engine as clap::ValueEnum>::value_variants()
        .iter()
        .map(|e| {
            let l = e.login();
            let token = l
                .token
                .map_or("none: --token".into(), |t| format!("`{}`", t.join(" ")));
            format!(
                "- {}: bound `{}`, {}; token {token}, in {}",
                e.name(),
                l.bound.join(" "),
                l.home_var,
                l.token_var.unwrap_or("none: bound only")
            )
        });
    lines.collect::<Vec<_>>().join("\n")
}

/// The manual: clap's command tree, and the constants it places by name.
fn manual() -> Help {
    Help::new(Cli::command())
        .table::<Code>("refusal")
        .constant("EXIT_USAGE", Code::Usage.exit())
        .constant("ENGINES", engines_help())
}

fn main() -> ExitCode {
    let help = manual();
    let parsed = help.clap().map_err(|e| e.to_string()).map(|command| {
        command
            .try_get_matches()
            .and_then(|m| Cli::from_arg_matches(&m))
    });
    let cli = match parsed {
        Err(broken) => return answer(Err(broken.into())),
        Ok(Ok(cli)) => cli,
        Ok(Err(e)) if !e.use_stderr() => e.exit(),
        Ok(Err(e)) => {
            let msg = e.render().to_string();
            return answer(Err(Refused::new(Code::Usage, msg.trim())));
        }
    };
    match cli.cmd {
        Cmd::Env { name, json } => answer(env(&name, json)),
        Cmd::Help { topic } => {
            let text = match topic {
                None => help.index(),
                Some(topic) => help.topic_help(&topic),
            };
            answer(text.map_err(|e| Refused::new(Code::Usage, e.to_string())))
        }
        Cmd::Register(cmd) => answer(run(cmd).map(|v| format!("{v}\n"))),
    }
}
/// Prints the answer, its text as it is, or the refusal as
/// {"error": …, "code": …}, exiting as its code says.
fn answer(result: Result<String, Refused>) -> ExitCode {
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(r) => {
            let code = agent_help::Table::entry(&r.code).name;
            println!("{}", json!({ "error": r.text, "code": code }));
            ExitCode::from(r.code.exit())
        }
    }
}

/// `env`'s answer: the account's variable as an `export NAME='value'` line,
/// the value one shell word whatever it holds (the text of factorium's
/// secrets file, factorium_harness::secrets_file), or with `json` as
/// {"name", "login", "env": {NAME: value}}.
fn env(name: &str, json: bool) -> Result<String, Refused> {
    let a = kept(&Store::open()?, name)?;
    let (var, value) = login::env(&a)?;
    Ok(if json {
        let env = json!({ var: value });
        format!(
            "{}\n",
            json!({ "name": a.name, "login": a.login(), "env": env })
        )
    } else {
        format!("export {var}='{}'\n", value.replace('\'', "'\\''"))
    })
}

/// An account's terms refused, each fault with its fix.
fn faults(f: Vec<String>) -> Refused {
    Refused::new(Code::BadTerms, f.join("; "))
}

fn shown(a: &Account) -> Value {
    json!({ "account": a.shown() })
}

fn now() -> Result<u64, String> {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map(|d| d.as_secs()).map_err(|e| e.to_string())
}

/// `a` as `show` answers it: with whether it is logged in.
fn with_status(a: &Account) -> Result<Value, Refused> {
    let today = (now()? / 86_400) as i64;
    let mut v = serde_json::to_value(account::status(a, login::logged_in(a)?, today))
        .map_err(|e| e.to_string())?;
    v["account"] = json!(a.shown());
    Ok(v)
}

/// The account `name`, or the refusal that says how to find one.
fn kept(store: &Store, name: &str) -> Result<Account, Refused> {
    store.get(name)?.ok_or_else(|| {
        Refused::new(Code::NoAccount, format!(
            "no account `{name}`: `stablehand account list` names those kept; add it with `stablehand account add {name} --engine <engine>`"
        ))
    })
}

fn run(cmd: Register) -> Result<Value, Refused> {
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
                    return Err(Refused::new(
                        Code::NameTaken,
                        format!(
                            "account `{name}` is already kept: change it with `stablehand account edit {name} …`, or remove it first with `stablehand account rm {name}`"
                        ),
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
                let a = login::login(&a, token, time::utc(now()?))?;
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
        Register::Off { name, reason } => {
            let mut a = kept(&store, &name)?;
            if reason.trim().is_empty() {
                let why = "--reason is empty: say why the account is off";
                return Err(Refused::new(Code::BadTerms, why));
            }
            a.off = Some(Off {
                by: std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
                at: time::utc(now()?),
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
        Register::Read { name, now: anyway } => {
            Ok(usage::read(&store, &chosen(&store, name)?, now()?, anyway)?)
        }
        Register::Limit {
            name,
            window,
            resets_at,
        } => usage::limit(&store, &kept(&store, &name)?, window, resets_at, now()?),
        Register::Status { name } => Ok(usage::status(&store, &chosen(&store, name)?, now()?)?),
        Register::Pick {
            engine,
            running,
            machine,
            stop_percent,
        } => {
            let all = store.list()?;
            if let Some((name, _)) = running
                .iter()
                .find(|(n, _)| !all.iter().any(|a| &a.name == n))
            {
                kept(&store, name)?;
            }
            let machine = match machine {
                Some(m) => m,
                None => login::this_machine()?,
            };
            let of: Vec<Account> = all.into_iter().filter(|a| a.engine == engine).collect();
            Ok(usage::pick(
                &store,
                &of,
                &running,
                &machine,
                stop_percent,
                now()?,
            )?)
        }
    }
}

/// The account `name`, or every account where none is named.
fn chosen(store: &Store, name: Option<String>) -> Result<Vec<Account>, Refused> {
    match name {
        Some(name) => kept(store, &name).map(|a| vec![a]),
        None => Ok(store.list()?),
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
