//! Where an account's login is, as stablehand finds it (docs/adr/0001
//! "stablehand runs no login and keeps no secret"): a bound login is its
//! engine's login file in its home, a portable one's token is what the
//! command it names prints. stablehand runs no engine's login and writes no
//! token: the person logs the engine in by the engine itself, and keeps a
//! token where they keep secrets.
use crate::account::Account;
use crate::bounded::{self, Ran};
use crate::refusal::{Code, Refused};
use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, UNIX_EPOCH};

/// This machine's name, as `--machine` names it: `$STABLEHAND_MACHINE` (not
/// empty), else `hostname -s`.
pub fn this_machine() -> Result<String, String> {
    if let Some(m) = std::env::var("STABLEHAND_MACHINE")
        .ok()
        .filter(|m| !m.is_empty())
    {
        return Ok(m);
    }
    let out = Command::new("hostname")
        .arg("-s")
        .output()
        .map_err(|e| format!("hostname -s: {e}: set STABLEHAND_MACHINE to this machine's name"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Commands that print a token kept where people keep secrets, as the
/// help and each refusal for a portable account not logged in name them.
pub const TOKEN_COMMANDS: &str = "the Keychain's `security find-generic-password -s <service> -w`, 1Password's `op read op://<vault>/<item>/<field>`, or mytokens' `mytokens get <entry> --field <field>`";

/// The line that logs `a` in, as a fix names it: bound, its engine's own
/// login with its home variable set to the account's home, run on its
/// machine; portable, the `account edit` that names its token's command.
pub fn login_line(a: &Account) -> String {
    let login = a.engine.login();
    let Some(machine) = &a.machine else {
        let make = login.token.map_or(String::new(), |t| {
            format!(
                "make a token with `{}`, keep it where you keep secrets, then ",
                t.join(" ")
            )
        });
        return format!(
            "{make}`stablehand account edit {} --token-command '<cmd>'`, <cmd> printing the token: {TOKEN_COMMANDS}",
            a.name
        );
    };
    let cmd = login.bound.join(" ");
    let line = match &a.home {
        Some(home) => format!("{}={home} {cmd}", login.home_var),
        None => cmd,
    };
    format!("`{line}` on {machine}")
}

/// Whether `a` is logged in: a bound login when its home on this machine
/// holds its engine's login file ([`login_file`]), a portable one when its
/// token command answers a token ([`token`]).
pub fn logged_in(a: &Account) -> Result<bool, String> {
    if a.machine.is_some() {
        return Ok(login_file(a)?.is_some());
    }
    Ok(a.token_command.is_some() && token(a).is_ok())
}

/// A bound login's file, as its engine's login wrote it in its home.
#[derive(Debug, Serialize)]
pub struct LoginFile {
    pub path: String,
    /// When it was last written (UTC, RFC 3339): when the account logged in.
    pub written_at: String,
    #[serde(skip)]
    pub written: u64,
}

/// The engine's login file in `a`'s home, where `a` is bound to this machine
/// and its home holds one: only looked at, never written.
pub fn login_file(a: &Account) -> Result<Option<LoginFile>, String> {
    let (Some(machine), Some(home)) = (&a.machine, &a.home) else {
        return Ok(None);
    };
    if &this_machine()? != machine {
        return Ok(None);
    }
    let path = in_home(a, home);
    let Some(written) = std::fs::metadata(&path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .and_then(|m| m.modified().ok())
    else {
        return Ok(None);
    };
    let written = written
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Ok(Some(LoginFile {
        path: path.display().to_string(),
        written_at: crate::time::utc(written),
        written,
    }))
}

/// Where `a`'s engine's login file is in `home`: the one list of login
/// files ([`crate::account::Login::file`]), read by [`login_file`] and
/// [`credential`] alike.
fn in_home(a: &Account, home: &str) -> std::path::PathBuf {
    Path::new(home).join(a.engine.login().file)
}

/// How long a token command may run, in seconds:
/// `$STABLEHAND_TOKEN_COMMAND_SECS`, else this.
pub const TOKEN_COMMAND_SECS: u64 = 20;

fn token_bound() -> Duration {
    let secs = std::env::var("STABLEHAND_TOKEN_COMMAND_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(TOKEN_COMMAND_SECS);
    Duration::from_secs(secs)
}

/// `a`'s portable token: the first line its token command prints, run by
/// `sh -c` with no input, its stderr dropped, bounded by [`token_bound`]
/// ([`bounded::run`]).
/// Refused, naming the command and never what it printed: no command, one
/// that did not start, exited non-zero, printed no token or ran past the
/// bound (it is killed, and whatever it started with it).
pub fn token(a: &Account) -> Result<String, String> {
    let name = &a.name;
    let Some(cmd) = &a.token_command else {
        return Err(format!(
            "account `{name}` is not logged in: it names no command that prints its token: {}",
            login_line(a)
        ));
    };
    let fault = |why: String| {
        format!(
            "account `{name}`'s token command `{cmd}` {why}: run it yourself to see why, or name another with `stablehand account edit {name} --token-command '<cmd>'`"
        )
    };
    let mut sh = Command::new("sh");
    sh.args(["-c", cmd])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let bound = token_bound();
    let (code, printed) = match bounded::run(sh, bound) {
        Err(e) => return Err(fault(format!("could not start ({e})"))),
        Ok(Ran::RanPast) => {
            return Err(fault(format!(
                "ran past {}s and was stopped",
                bound.as_secs()
            )));
        }
        Ok(Ran::Exited(code, printed)) => (code, printed),
    };
    match code {
        Some(0) => {}
        Some(c) => return Err(fault(format!("exited {c}"))),
        None => return Err(fault("was killed".into())),
    }
    let text = String::from_utf8_lossy(&printed);
    match text.lines().next().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => Ok(t.to_string()),
        None => Err(fault("printed no token on its first line".into())),
    }
}

/// An account's login as a session's caller is handed it: the session's home
/// is the caller's (docs/adr/0001 "stablehand the login, the caller the
/// session").
pub enum Credential {
    /// A portable login: the engine's token variable and the token.
    Token { var: &'static str, token: String },
    /// A bound login: its engine's login files in the account's home, for
    /// the caller to link into the session's own home.
    Files(Vec<Link>),
}

/// One login file: where it is in the account's home, and where the
/// engine's home holds it.
#[derive(Debug, Serialize)]
pub struct Link {
    pub path: String,
    #[serde(rename = "as")]
    pub as_: &'static str,
}

/// `a`'s credential on this machine; refused `no_session`, with its fix, an
/// account no session may start on here: off, of an engine with no token
/// variable, not logged in (its token command answers none, its home holds
/// no login file), with no home, or bound to another machine.
pub fn credential(a: &Account) -> Result<Credential, Refused> {
    let no_session = |text: String| Err(Refused::new(Code::NoSession, text));
    let name = &a.name;
    if let Some(off) = &a.off {
        return no_session(format!(
            "account `{name}` is off ({} at {}: {}): start on another account, or `stablehand on {name}`",
            off.by, off.at, off.reason
        ));
    }
    let login = a.engine.login();
    let Some(machine) = &a.machine else {
        let Some(var) = login.token_var else {
            return no_session(format!(
                "{} takes no token from its environment: bind account `{name}` with `stablehand account edit {name} --machine <machine> --home <dir>`, then log it in there with `{}`",
                a.engine.name(),
                login.bound.join(" ")
            ));
        };
        return match token(a) {
            Ok(token) => Ok(Credential::Token { var, token }),
            Err(why) => no_session(why),
        };
    };
    let Some(home) = &a.home else {
        return no_session(format!(
            "account `{name}` has no home: `stablehand account edit {name} --home <dir>`, then log it in with {}",
            login_line(a)
        ));
    };
    let here = this_machine()?;
    if &here != machine {
        return no_session(format!(
            "account `{name}` is bound to {machine} and this machine is {here} ($STABLEHAND_MACHINE, else `hostname -s`): start its session on {machine}, or on another account"
        ));
    }
    if !logged_in(a)? {
        return no_session(format!(
            "account `{name}` is not logged in: its home holds no {}: log it in with {}",
            login.file,
            login_line(a)
        ));
    }
    Ok(Credential::Files(vec![Link {
        path: in_home(a, home).display().to_string(),
        as_: login.file,
    }]))
}
