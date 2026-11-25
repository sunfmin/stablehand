//! stablehand: one home for agent engine accounts, their logins and their
//! usage windows (README.md, docs/adr/0001): the account register and the books.
mod account;
mod books;
mod bounded;
mod command;
mod curl;
mod login;
mod order;
mod pick;
mod readers;
mod refusal;
mod run;
mod scan;
mod statusline;
mod store;
mod time;
mod usage;
// build.rs runs it to stamp the version; compiled here too, for its tests.
#[cfg_attr(not(test), allow(dead_code))]
mod version;

use account::{Account, Change, Engine, Field, Off, Terms};
use agent_help::Help;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use login::Credential;
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
/// stablehand runs no login and keeps no secret: a person logs an engine in by the engine itself, and stablehand finds the login where it is. A bound account is logged in when its home holds its engine's login file; a portable one when the command it names (`--token-command`) prints its token, the token kept where the person keeps secrets (the Keychain, 1Password, mytokens).
///
/// `stablehand credential <name>` hands a session's caller an account's login (a token, or the login files to link into the session's own home); `stablehand read` asks an account's engine for its windows and books the reading, `stablehand limit` books a session's report that it met the account's limit, `stablehand status` says what the books say now, each window's pace with it, and `stablehand pick` what the next session starts on: the first engine, model and account of the order (`stablehand order`) one more session may start on. The session itself, its command, flags and home, is the caller's.
///
/// Start with `stablehand account list`. Run `stablehand <command> --help` before a command's first run.
///
/// ## Rules to know first
///
/// - Every command but `credential` of a portable login prints one JSON object on stdout. A refusal is `{"error": "…", "code": "…"}`, each fault with the line that fixes it, exit 1, and nothing written; a command line stablehand cannot read is the same with exit {const:EXIT_USAGE}. `--help`, `help` and `--version` answer text.
/// - `stablehand statusline` answers the status bar's line: it is Claude Code's status line command, not one to run by hand.
/// - `stablehand credential <name>` of a portable login answers shell text, an `export` line (`--json`: one object); it refuses an account no session may start on with exit {const:EXIT_USAGE}.
/// - The register and the books are one SQLite file, `$STABLEHAND_CONFIG_DIR/stablehand.db` (default `~/.config/stablehand`). It keeps no token: of a portable login it keeps the command that prints it. The default is a released stablehand's only: a development build (`cargo build`, any version with `+`) refuses unless `STABLEHAND_CONFIG_DIR` is set, as its schema steps may not be the released ones. A store migrated by other steps than this stablehand's is refused, naming the step.
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
    /// An account's login, as a session's caller is handed it: a token, or the login files to link. It only reads, and logs nothing in.
    ///
    /// The session's home is the caller's: it sets the engine's home variable to a home of its own and links the login into it, so nothing else of the account's home (a person's settings, hooks, skills) reaches the session. Portable: the engine's token variable (below), its value the first line the account's `--token-command` prints, run by `sh -c` with no input, its stderr dropped, stopped past {const:TOKEN_COMMAND_SECS}s (`$STABLEHAND_TOKEN_COMMAND_SECS`). Bound: the engine's login file (below) in the account's `--home`, on its machine only (`$STABLEHAND_MACHINE`, else `hostname -s`).
    ///
    /// ## What it answers
    ///
    /// Portable: one `export NAME='value'` line, the value one shell word, for a shell to read (`. <file>`, or `eval`); with `--json`, `{"name", "login": "portable", "env": {NAME: value}}`. Bound, with or without `--json`: `{"name", "login": "bound", "files": [{"path", "as"}]}`, `path` the file in the account's home and `as` where the engine's home holds it, for the caller to link `<its home>/<as>` to `path`. Written to stdout alone: no file, and no child is given a value on its command line.
    ///
    /// ## What it refuses
    ///
    /// Exit {const:EXIT_USAGE}, with the fix: an account off, not logged in (its home holds no login file: the fix is its engine's login line, `CODEX_HOME=<home> codex login`; it names no token command, or its command exits non-zero, prints no first line or runs past the bound: the refusal names the command, never what it printed), bound to another machine or with no home, and a portable one of an engine with no token variable. Exit 1: no such account.
    ///
    /// ## Engines
    ///
    /// {const:ENGINES}
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
    Credential {
        name: String,
        /// Answer one JSON object instead of a portable login's `export` line.
        #[arg(long, verbatim_doc_comment)]
        json: bool,
    },
    /// Start an engine's own command on the account, and model, stablehand picks, handing it the login: `stablehand run claude -p hi`.
    ///
    /// The engine is the command's (`claude` is claude-code's: {const:PROGRAMS}), else `--engine`'s, else `--account`'s. With no command, the whole order is walked (`stablehand order show`) and the picked engine's own program starts, interactive: `stablehand run`. The engine, model and account are `stablehand pick`'s (`--engine`, `--account`, `--model` narrow it; a model the command names, `--model opus`, narrows it too).
    ///
    /// ## What the child gets
    ///
    /// - Its login, in its environment alone, never on a command line: portable, the engine's token variable (`CLAUDE_CODE_OAUTH_TOKEN`) set to what the account's `--token-command` prints; bound, the engine's home variable (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `HOME` for agy, …) set to the account's home, the token variable cleared. With `--home <dir>`, the home variable is `<dir>`, and a bound account's login files (`stablehand credential`) are linked into it.
    /// - The account's name, never a value, in `{const:ACCOUNT_VAR}`: its status line (`stablehand statusline`) books and shows that account's windows.
    /// - The model: where the command names none, the engine's model flag and the picked model are put after its program (`claude --model opus -p hi`). dsh takes none: its profile names it.
    /// - The terminal, as stablehand's. stablehand waits for it: its exit code is stablehand's, and a signal that ends it ends stablehand the same way. A signal a process sends stablehand (`kill -TERM`) is passed on to it; the terminal's (^C) reach it themselves.
    ///
    /// stderr gets one line of what was chosen, `stablehand: claude-code on claude-felix (opus)`, never a value. While the child runs, its session is kept in the register, and `stablehand pick` counts it beside `--running`; it is dropped when the child ends, or found dead by its pid.
    ///
    /// ## What it refuses
    ///
    /// Nothing started, `{"error", "code"}` on stdout: none picked (`none_picked`, exit 1, with `pick`'s `until` and `why`); arguments with no command (`stablehand run -- -p hi`: they mean something else to each engine), a command of no engine without `--engine`, engines that differ, `--home` for a bound claude-code account (Claude Code replaces a linked login with a copy when it refreshes it, docs/adr/0002), a `--home` holding a login file that is no link to the account's (`bad_run`, exit 1); an account no session may start on (`no_session`, exit {const:EXIT_USAGE}).
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
    Run(run::Asked),
    /// Claude Code's status line command: books the session's rate limits as a reading of the account.
    ///
    /// Claude Code runs it with its session's JSON on stdin; its `rate_limits` (`five_hour`, `seven_day`: `used_percentage`, `resets_at`) are booked as a reading of `--account` (else `${const:ACCOUNT_VAR}`, which `stablehand run` sets in the session it starts), as `stablehand read` books one, when they differ from the books' latest entry. It asks no engine and reaches no network. Set it in `~/.claude/settings.json`:
    ///
    /// `"statusLine": {"type": "command", "command": "stablehand statusline --account claude-felix"}`
    ///
    /// or, for each session `stablehand run` starts to show its own account, and any other its person's line alone:
    ///
    /// `"statusLine": {"type": "command", "command": "stablehand statusline --then ~/.claude/statusline.sh"}`
    ///
    /// ## What it answers
    ///
    /// One line for the status bar, `claude-felix · 5h 24% · 7d 41%`, exit 0. With no `rate_limits` given (a `claude setup-token` session is given none, nor is one before its first answer), the windows are the books' latest, as `stablehand status` shows them, each with its age past ten minutes (`7d 70% (3h ago)`, `reset` once it has reset since); an account with none booked, its name alone. With no account named, nothing is booked and the line is empty (with `--then`, the person's line alone). With `--then <cmd>`, `sh -c <cmd>` is given the same stdin, byte for byte, and its stdout and exit are the answer: a person's own status line keeps working.
    ///
    /// ## What trips you up
    ///
    /// It never refuses, so the bar always has a line: an account not kept, one not claude-code's, or a register it cannot open books nothing, and the line says why (with `--then`, stderr does).
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
    Statusline {
        /// The claude-code account the session runs on; default `${const:ACCOUNT_VAR}`, which `stablehand run` sets.
        #[arg(long, verbatim_doc_comment)]
        account: Option<String>,
        /// The person's own status line command (`~/.claude/statusline.sh`), run by `sh -c` on the same stdin; its line is printed instead.
        #[arg(long, verbatim_doc_comment)]
        then: Option<String>,
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
    /// Add, find (scan), list, show, change or remove an account.
    #[command(next_help_heading = REGISTER, verbatim_doc_comment)]
    Account {
        #[command(subcommand)]
        cmd: AccountCmd,
    },
    /// Set or show the order: the engines, and of each the models, the next session starts on, highest first (`stablehand pick` walks it).
    #[command(next_help_heading = REGISTER, verbatim_doc_comment)]
    Order {
        #[command(subcommand)]
        cmd: OrderCmd,
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
    /// Read an account's usage windows, or its balance, from its engine, and book the reading.
    ///
    /// Without a name, every account. An account is read at most once an hour: one read or tried in the last hour waits, unless `--now`. A reader's failure is booked as a failed try and said; the account's last windows and balances are kept, marked stale. A credential the engine refused (dsh: HTTP 401 or code 40003) is never asked again, `--now` or not, until the account logs in again: its home's login file written after it, or a portable one given its `--token-command` again (`stablehand account edit`). A failed or refused read still exits 0.
    ///
    /// ## What it answers
    ///
    /// `{"reads": [{"account", "outcome", "windows", "balances", …}]}`, `outcome` one of `read`, `failed` (with `error`, the reason and its fix), `refused` (with `error`: the engine refused its credential, now or before, and nothing is asked until it logs in again), `waits` (with `next_at`) or `no_reader` (its engine has no reader yet); each window and balance as `stablehand status` shows it. A `read` says `read_by` (`usage`, `question`, or null where its engine asks one way) and `spent_turn`: true when the reading spent one model turn of the account's quota (`question`).
    ///
    /// ## Engines
    ///
    /// claude-code is read for its windows: `GET api.anthropic.com/api/oauth/usage` (`read_by: "usage"`, no turn), the bearer a portable login's from its `--token-command`, a bound one's from `<home>/.credentials.json`. A token it refuses (HTTP 403 scope: a `claude setup-token` token) is asked the question instead (`read_by: "question"`): `claude -p --model haiku --output-format stream-json --verbose --tools '' --strict-mcp-config --no-session-persistence --setting-sources '' --system-prompt 'Answer with one word.' quota`, the token in its `CLAUDE_CODE_OAUTH_TOKEN`, in a new empty config dir (`CLAUDE_CONFIG_DIR`, its working directory too) removed after, so nothing of `~/.claude` is read; its stream's `rate_limit_event` says the windows. An account read by the question is asked the question again, not its usage. dsh, pay-as-you-go, is read for its balance and has no window: `GET platform.deepseek.com/api/v0/users/get_user_summary` with the grant dsh keeps (`<home>/.credentials.yaml`, its `DSH_HOME`; `~/.dsh` without `--home`), or a portable login's from its `--token-command`.
    ///
    /// ## What trips you up
    ///
    /// A claude-code account whose login is a `claude setup-token` token spends one smallest-model turn of its quota a reading (about 3s, under $0.0002): at most once an hour, unless `--now`, and each such read says `spent_turn: true`.
    ///
    /// An agy account is read by running agy in its home, on its machine only: a portable one is refused (bind it), and a home not signed in fails only after agy has waited 60s for a browser sign-in. After its quota, `agy models` names each group's models; where it fails, the windows are booked with none, and `pick --model` judges the account by every window.
    /// A codex or pi account is read through its engine's own login, bound: on its machine, in its home. A portable one, or one bound to another machine, fails with the fix.
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
    Read {
        /// The account to read; without it, every account.
        #[arg(verbatim_doc_comment)]
        name: Option<String>,
        /// Read even if it was read or tried in the last hour. A claude-code `claude setup-token` account's reading spends one small model turn.
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
    /// `{"accounts": [{"name", "engine", "off", "pace", "windows", "balances", "limit", "read_at", "read_by", "tried_at", "failed", "refused", "read_next"}]}`. `read_by` is how the latest reading was asked, where its engine asks two ways (claude-code): `usage` (no turn) or `question` (one smallest-model turn spent: a `claude setup-token` token); null otherwise. `refused` is true when the last try's credential was refused and the account has not logged in since: it is read again only after it logs in again (its home's login file written since, or `stablehand account edit --token-command`), whatever `read_next` says. Each balance (a pay-as-you-go account's, dsh's; such an account has no window) is `{"wallet" ("normal" or "bonus"), "currency", "amount", "at", "stale" (a later try failed)}`. Each window is `{"name", "group" (null where the engine groups none), "models" (the ids of the models whose use it counts, its group's: agy's; empty where the engine names none), "used_percent" (null once it has reset since it was read), "resets_at", "source" ("read" or "limit"), "at" (when that was booked), "stale" (a later try failed, or it has reset since), "pace"}`.
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
    /// What the next session starts on: the first engine, model and account of the order one more session may start on now.
    ///
    /// Reads no engine: it judges the books as `stablehand status` shows them (run `stablehand read` first for fresh windows). It walks the order (`stablehand order show`): each engine in turn, and of each its models in turn (the engine's own where the order lists none); at each (engine, model), the engine's accounts usable on this machine (portable, or bound to it), logged in, and not off, judged by the rules below. An engine of the order with no account kept is skipped, and said (`no_account`): nothing starts on a login stablehand does not keep.
    ///
    /// ## Narrowing the walk
    ///
    /// `--engine` (repeatable) and `--model` narrow the walk to them, in the order's order. An engine or a model not in the order may still be named: it is tried after the rest, an engine named without `--model` on its own model. With neither, and no order kept, there is nothing to walk: it refuses (`no_order`).
    ///
    /// ## The rules
    ///
    /// Guild's ledger's, carried over. None is picked on an account
    ///
    /// - a caller's limit holds on (`stablehand limit`), until it lifts;
    /// - any of whose windows is `--stop-percent` used, until that window resets: the rest is the margin of the sessions already running;
    /// - one of whose windows, at its present rate shared by the sessions running and one more, runs out before it resets. A reading over two hours old, or a window under half used, runs on at no rate.
    ///
    /// Of the rest, the one with the most left of its most used window; one with no reading in force, or only stale ones, after every one with one; of equals, the first by name.
    ///
    /// ## A session's model
    ///
    /// At a step with a model, an account whose windows are grouped by model (agy: `Gemini Models`, `Claude and GPT models`) is judged only by the windows of the group holding that model (its windows' `models` in `stablehand status`), and those in no group: every rule above, a limit on a window of another group left out. So one agy account is two pools: its Gemini group used up, the walk falls to its Claude model. One no group of which holds the model is judged by every window, and named in the answer's `model_in_no_group`. An engine that groups no window is judged as without a model.
    ///
    /// ## What it answers
    ///
    /// `{"engine", "model" (null: the engine's own), "account", "why", "model_in_no_group"}`, or `{"none": true, "until": <the earliest time any passed over lifts, or null>, "why", "model_in_no_group"}`. `why` is each (engine, model, account) passed over before the answer, in the walk's order: `[{"engine", "model", "account" (null for an engine skipped), "reason", "why", "until"}]`, `reason` one of `no_account`, `off`, `elsewhere`, `not_logged_in`, `limit`, `stopped`, `runs_out`. It refuses a `--running` name that is no account kept, and a walk with nothing in it.
    ///
    /// The order is a preference only: the command an engine starts with, its flags, and which caller keeps to which engine stay the caller's.
    ///
    /// ## What trips you up
    ///
    /// `--running` is the caller's count, on this machine or wherever it knows; the sessions `stablehand run` keeps running are counted beside it, and no other. An account left out of both has none running.
    #[command(next_help_heading = SESSIONS, verbatim_doc_comment)]
    Pick {
        /// Walk only this engine's steps; repeatable. One not in the order is tried after the rest.
        #[arg(long, value_enum, verbatim_doc_comment)]
        engine: Vec<Engine>,
        /// Walk only the steps of this model, by the id its engine starts it with (`agy models`: `claude-opus-5-5-high`); one not in the order is tried on each `--engine` after the rest.
        #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new(), verbatim_doc_comment)]
        model: Option<String>,
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

/// The order: which engines, and of each which models, the next session starts on.
#[derive(Subcommand)]
enum OrderCmd {
    /// Set the order, highest first: each `<engine>[@<model>[,<model>…]]`.
    ///
    /// One order per machine, kept in the register's database; it replaces the one kept. An engine is one of stablehand's (below); its models are the ids its engine starts a session with, highest first (claude-code's `opus`), none naming the engine's own. agy's are checked against the ids `agy models` lists, run in the home of the first agy account bound to this machine (the user's own without one). The order is a preference only: the command an engine starts with, its flags and its guard stay the caller's.
    ///
    /// `stablehand order set claude-code@opus codex grok agy@gemini-4-neon-high,claude-opus-5-5-high`
    ///
    /// ## What it answers
    ///
    /// `{"order": [{"engine", "models": [...]}]}`, as `stablehand order show` does. It refuses, writing nothing (`bad_order`), an engine it does not know, one given twice, an empty model or one given twice, and an agy model `agy models` does not list, or `agy models` failing.
    ///
    /// ## Engines
    ///
    /// {const:ENGINES}
    #[command(verbatim_doc_comment)]
    Set {
        /// `<engine>[@<model>[,<model>…]]`, highest first.
        #[arg(required = true, verbatim_doc_comment)]
        ranks: Vec<String>,
    },
    /// The order kept, highest first.
    ///
    /// Answers `{"order": [{"engine", "models": [...]}]}`, empty when none is set.
    #[command(verbatim_doc_comment)]
    Show,
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
    /// Answers `{"account": {"name", "engine", "login" ("portable" or "bound"), "machine", "expires", "home", "off", "token_command", "identity" (the hash of the identity `stablehand account scan` read in its login)}}`, a field it lacks null. Refuses, writing nothing, a name already kept, an unknown engine (exit {const:EXIT_USAGE}), a date that is no YYYY-MM-DD day, `--expires` or `--token-command` with `--machine` (a bound login is its engine's files, no token) and `--home` without `--machine`.
    ///
    /// ## Logging in
    ///
    /// stablehand runs no login. A bound account is logged in by its engine itself, in its home, on its machine (below: `CODEX_HOME=<home> codex login`). A portable one names the command that prints its token, kept where the person keeps secrets: {const:TOKEN_COMMANDS}; claude-code's token is made by `claude setup-token`.
    ///
    /// ## Engines
    ///
    /// {const:ENGINES}
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
    /// Answers `{"account": {…}, "logged_in": true|false, "days_left": n|null, "warning": "…"|null}`: a bound login is logged in when its home on this machine holds its engine's login file (below), logged in when that was written; a portable one when its `--token-command` prints a token (it is run, and the token shown nowhere); `days_left` counts to `--expires`, and `warning` says from 30 days ahead to make a new token; `login_file` is `{"path", "written_at"}`, the engine's login file in a bound account's home and when it was last written, or null where it holds none or the account is not bound to this machine. The file is only looked at, never written.
    ///
    /// ## Engines
    ///
    /// {const:ENGINES}
    #[command(verbatim_doc_comment)]
    Show { name: String },
    /// Change an account's terms: only those given change.
    ///
    /// Answers `{"account": {…}}`; refuses as `stablehand account add` does, judging the account as it would be. Giving `--token-command` is a new login: a credential its engine refused is asked again.
    #[command(verbatim_doc_comment)]
    Edit {
        name: String,
        /// Its engine.
        #[arg(long, value_enum, verbatim_doc_comment)]
        engine: Option<Engine>,
        #[command(flatten)]
        terms: Terms,
        /// Clear a term: machine (the login becomes portable), expires, home, token-command. Repeatable.
        #[arg(long, value_enum, verbatim_doc_comment)]
        unset: Vec<Field>,
    },
    /// Find the engine logins and the portable tokens already on this machine; with `--add`, register them, one account per subscription.
    ///
    /// It looks in each engine's own default home under `$HOME` ({const:DEFAULT_HOMES}), and in each `--path`; never in a home another program keeps for its own use (factorium's `~/.agent-accounts`, Guild's `~/.guild/engines`) unless a `--path` names it. A home is a login when it holds its engine's login file (below). It also reads `mytokens list`, when mytokens is on PATH: each `agent-account-<name>` entry with an engine's token field (`CLAUDE_CODE_OAUTH_TOKEN`) is a portable account, its `--token-command` `mytokens get agent-account-<name> --field <that field>`; one with no token (a bound one, its `MACHINE`) is skipped. No token is read.
    ///
    /// ## One account per subscription
    ///
    /// The same subscription is often logged in in several homes (pi's openai-codex and codex on one ChatGPT account). scan tells them apart by the identity each login file holds ({const:IDENTITIES}), read and compared in memory and never said: it answers, and keeps on an account it adds, the identity's hash. Of the logins of one identity, the account is the one an account already kept is (kept at that home, or holding that hash), else the most recently written; the others are copies of it. An identity it cannot read is said so (`identity_unknown`), and that login is never a copy. A portable token tells no identity (a setup token is refused `user:profile`): whether it is a home login's subscription is `unknown`.
    ///
    /// ## What it answers
    ///
    /// `{"add", "machine", "logins": [{"engine", "home", "logged_in_at", "identity", "identity_unknown", "status", "account", "warning"}], "tokens": [{"entry", "engine", "token_command", "status", "account", "same_as_a_login"}], "mytokens_unread"}`. `status` is `new` (`account` the name `--add` gives it: `<engine>`, or `<engine>-2` … when the name is kept; a token its entry's `<name>`), `added` (with `--add`), `registered` (`account` is it), `copy` (of `account`; `warning` says why to run sessions on that one only) or, for a token entry, `skipped` (with `why`). `mytokens_unread` says why mytokens was not read, null when it was.
    ///
    /// ## What it changes
    ///
    /// Without `--add`, nothing. With `--add`, it adds each `new` login as `stablehand account add <name> --engine <engine> --machine <this machine> --home <its home>` would, its identity's hash kept, and each `new` token as a portable account with its token command; nothing else: an account kept is never renamed or changed, nothing is written in a home, mytokens is only listed. Run again, it adds nothing more. It refuses a `--path` that is no directory.
    ///
    /// ## Engines
    ///
    /// {const:ENGINES}
    #[command(verbatim_doc_comment)]
    Scan {
        /// Register each `new` login and token.
        #[arg(long, verbatim_doc_comment)]
        add: bool,
        /// One more home to look in, for each engine whose login file it holds. Repeatable.
        #[arg(long, verbatim_doc_comment)]
        path: Vec<String>,
    },
    /// Remove an account and its books.
    ///
    /// Answers `{"name": …, "removed": true}`, or false when there was none: run twice, the same register.
    #[command(verbatim_doc_comment)]
    Rm { name: String },
}

/// Each engine's login, as `stablehand credential --help`, `stablehand account add --help` and `stablehand account show --help` list it.
fn engines_help() -> String {
    let lines = <Engine as clap::ValueEnum>::value_variants()
        .iter()
        .map(|e| {
            let l = e.login();
            let token = l
                .token
                .map_or("none made".into(), |t| format!("made by `{}`", t.join(" ")));
            format!(
                "- {}: bound, logged in by `{}` with {} its home; portable token {token}, in {}; login file `{}`",
                e.name(),
                l.bound.join(" "),
                l.home_var,
                l.token_var.unwrap_or("none: bound only"),
                l.file
            )
        });
    lines.collect::<Vec<_>>().join("\n")
}

/// Each engine's default home, as `stablehand account scan --help` lists it.
fn default_homes() -> String {
    let homes = <Engine as clap::ValueEnum>::value_variants()
        .iter()
        .map(|e| {
            let home = e.login().default_home;
            let home = if home.is_empty() {
                "~".into()
            } else {
                format!("~/{home}")
            };
            format!("{} `{home}`", e.name())
        });
    homes.collect::<Vec<_>>().join(", ")
}

/// The manual: clap's command tree, and the constants it places by name.
fn manual() -> Help {
    Help::new(Cli::command())
        .table::<Code>("refusal")
        .constant("EXIT_USAGE", Code::Usage.exit())
        .constant("ENGINES", engines_help())
        .constant("PROGRAMS", command::programs())
        .constant("TOKEN_COMMANDS", login::TOKEN_COMMANDS)
        .constant("DEFAULT_HOMES", default_homes())
        .constant("IDENTITIES", scan::IDENTITIES)
        .constant("TOKEN_COMMAND_SECS", login::TOKEN_COMMAND_SECS)
        .constant("ACCOUNT_VAR", statusline::ACCOUNT_VAR)
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
        Cmd::Credential { name, json } => answer(credential(&name, json)),
        Cmd::Run(asked) => {
            match now()
                .map_err(Refused::from)
                .and_then(|now| run::run(asked, now))
            {
                Ok(code) => code,
                Err(r) => answer(Err(r)),
            }
        }
        Cmd::Statusline { account, then } => statusline::run(account, then.as_deref()),
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

/// `credential`'s answer: a portable login's token as an `export NAME='value'`
/// line, the value one shell word whatever it holds (the text of factorium's
/// secrets file, factorium_harness::secrets_file), or with `json` as
/// {"name", "login", "env": {NAME: value}}; a bound login's files as
/// {"name", "login", "files": [{"path", "as"}]}.
fn credential(name: &str, json: bool) -> Result<String, Refused> {
    let a = kept(&Store::open()?, name)?;
    let answer = |key: &str, value: Value| {
        let mut answer = json!({ "name": a.name, "login": a.login() });
        answer[key] = value;
        format!("{answer}\n")
    };
    Ok(match login::credential(&a)? {
        Credential::Token { var, token } if !json => {
            format!("export {var}='{}'\n", token.replace('\'', "'\\''"))
        }
        Credential::Token { var, token } => answer("env", json!({ var: token })),
        Credential::Files(files) => answer("files", json!(files)),
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
    v["login_file"] = json!(login::login_file(a)?);
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
            AccountCmd::Edit {
                name,
                engine,
                terms,
                unset,
            } => {
                let given_command = terms.token_command.is_some();
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
                if given_command {
                    // A new login: a credential its engine refused is asked again.
                    let what = books::What::LoggedIn;
                    store.book(&a.name, &books::Entry { at: now()?, what })?;
                }
                Ok(shown(&a))
            }
            AccountCmd::Scan { add, path } => scan::scan(&store, &path, add),
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
            model,
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
            let ask = usage::Ask {
                engines: engine,
                model,
                running,
                machine,
                stop_percent,
            };
            usage::pick_now(&store, &all, ask, now()?)
        }
        Register::Order { cmd } => {
            if let OrderCmd::Set { ranks } = cmd {
                let bad = |f: Vec<String>| Refused::new(Code::BadOrder, f.join("; "));
                let order = order::parse(&ranks).map_err(bad)?;
                if let Some(agy) = order
                    .iter()
                    .find(|r| r.engine == Engine::Agy && !r.models.is_empty())
                {
                    let listed =
                        readers::agy::models(&agy_home(&store)?).map_err(|e| bad(vec![e]))?;
                    let unknown: Vec<String> = agy
                        .models
                        .iter()
                        .filter(|m| !listed.contains(m))
                        .map(|m| {
                            format!(
                                "`agy models` lists no `{m}`: give one of {}",
                                listed.join(", ")
                            )
                        })
                        .collect();
                    if !unknown.is_empty() {
                        return Err(bad(unknown));
                    }
                }
                store.set_order(&order)?;
            }
            Ok(json!({ "order": store.order()? }))
        }
    }
}

/// Where `agy models` runs: the home of the first agy account bound to this
/// machine, else the user's own.
fn agy_home(store: &Store) -> Result<String, Refused> {
    let here = login::this_machine()?;
    let home = store
        .list()?
        .into_iter()
        .find(|a| a.engine == Engine::Agy && a.machine.as_deref() == Some(here.as_str()))
        .and_then(|a| a.home);
    Ok(home.unwrap_or_else(|| std::env::var("HOME").unwrap_or_default()))
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
