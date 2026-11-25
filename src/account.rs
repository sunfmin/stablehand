//! What an account is and which of its terms may be kept: the register's
//! decisions, with no I/O. The store keeps what these functions accept; the
//! command line hands them what the person typed.
use serde::{Deserialize, Serialize};

/// An agent program a session runs in (GLOSSARY.md "Engine"). The one list of
/// them: `--engine`'s values, its help and its refusal are derived from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    ClaudeCode,
    Codex,
    Pi,
    Kimi,
    Agy,
    Grok,
    Dsh,
}

impl Engine {
    /// Its name, as `--engine` takes it.
    pub fn name(self) -> String {
        let value = clap::ValueEnum::to_possible_value(&self).expect("every engine is a value");
        value.get_name().to_string()
    }

    /// The line that starts a session of it, interactive (`stablehand run`
    /// with no command): its program first, the name `run` knows it by.
    pub fn start(self) -> &'static [&'static str] {
        use Engine::*;
        match self {
            ClaudeCode => &["claude"],
            Codex => &["codex"],
            Pi => &["pi"],
            Kimi => &["kimi"],
            Agy => &["agy"],
            Grok => &["grok"],
            // dsh boots a profile: its terminal one.
            Dsh => &["dsh", "tui"],
        }
    }

    /// The flags that name a session's model, the first the one `run` adds;
    /// none: the engine takes no model on its command line (dsh's is its
    /// profile's).
    pub fn model_flags(self) -> &'static [&'static str] {
        use Engine::*;
        match self {
            ClaudeCode | Pi | Agy => &["--model"],
            Codex | Kimi | Grok => &["--model", "-m"],
            Dsh => &[],
        }
    }

    /// The engine whose program is `program` (a path's last part): `claude`
    /// is claude-code's.
    pub fn of_program(program: &str) -> Option<Engine> {
        let name = std::path::Path::new(program).file_name()?.to_str()?;
        <Engine as clap::ValueEnum>::value_variants()
            .iter()
            .copied()
            .find(|e| e.start()[0] == name)
    }

    /// How the engine logs in, its own way (docs/adr/0001 "Logins are the
    /// engine's own"): the one table of each engine's login facts.
    pub fn login(self) -> Login {
        use Engine::*;
        let (home_var, bound, token): (_, &[&str], Option<&[&str]>) = match self {
            ClaudeCode => (
                "CLAUDE_CONFIG_DIR",
                &["claude", "auth", "login"],
                Some(&["claude", "setup-token"]),
            ),
            Codex => ("CODEX_HOME", &["codex", "login"], None),
            Pi => ("PI_CODING_AGENT_DIR", &["pi"], None),
            Kimi => ("KIMI_CODE_HOME", &["kimi", "login"], None),
            Agy => ("HOME", &["agy"], None),
            Grok => ("GROK_HOME", &["grok", "login"], None),
            Dsh => ("DSH_HOME", &["dsh", "tui"], None),
        };
        let file = match self {
            ClaudeCode => ".credentials.json",
            Codex | Pi | Grok => "auth.json",
            Kimi => "credentials/kimi-code.json",
            Dsh => ".credentials.yaml",
            Agy => ".gemini/antigravity-cli/antigravity-oauth-token",
        };
        // Where the engine keeps its home when its home variable is unset,
        // under $HOME: where `account scan` looks.
        let default_home = match self {
            ClaudeCode => ".claude",
            Codex => ".codex",
            Pi => ".pi/agent",
            Kimi => ".kimi-code",
            Agy => "",
            Grok => ".grok",
            Dsh => ".dsh",
        };
        Login {
            home_var,
            default_home,
            bound,
            token,
            file,
            // The others read a token from where their own login put it, as
            // far as stablehand knows: their sessions start bound.
            token_var: (self == ClaudeCode).then_some("CLAUDE_CODE_OAUTH_TOKEN"),
        }
    }
}

/// An engine's login facts. agy, pi and dsh have no login command: started
/// bare in their home, they sign in on first use.
pub struct Login {
    /// The variable the engine reads for its home.
    pub home_var: &'static str,
    /// Its home without that variable, under `$HOME` (agy's is `$HOME` itself).
    pub default_home: &'static str,
    /// The command that logs a bound account in, run in its home.
    pub bound: &'static [&'static str],
    /// The file its login writes in its home: a bound home holding it is
    /// logged in.
    pub file: &'static str,
    /// The command that makes a long-lived token, where the engine has one:
    /// the person runs it and keeps the token where they keep secrets.
    pub token: Option<&'static [&'static str]>,
    /// The variable a session reads a portable token from, where the engine has one.
    pub token_var: Option<&'static str>,
}

/// Who switched an account off, when (UTC, RFC 3339) and why
/// (GLOSSARY.md "Off").
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Off {
    pub by: String,
    pub at: String,
    pub reason: String,
}

/// One account as the register keeps it (GLOSSARY.md "Account", "Login").
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Account {
    pub name: String,
    pub engine: Engine,
    /// The machine its bound login lives on; none: its login is portable.
    pub machine: Option<String>,
    /// When its portable token expires, `YYYY-MM-DD`.
    pub expires: Option<String>,
    /// Its bound login's home on `machine`.
    pub home: Option<String>,
    pub off: Option<Off>,
    /// The command whose first line printed is its portable login's token
    /// (`mytokens get …`): the token stays where the person keeps it, and
    /// stablehand keeps only how to read it (docs/adr/0001).
    pub token_command: Option<String>,
    /// The hash of the identity its login holds, as `account scan` read it
    /// (the identity itself is kept nowhere): scan tells by it which login
    /// found later is this account's. A portable claude-code token's is
    /// its organization's.
    pub identity: Option<String>,
    /// A bound claude-code login's organization, beside its identity: the
    /// hash of its `organizationUuid`, which a token of it names
    /// ([`copy_of`]), and its `organizationType`.
    pub organization: Option<String>,
    pub organization_type: Option<String>,
}

/// Whether a Claude organization of type `kind` is one person's, so its
/// subscription: `claude_max`, `claude_pro`. A team's is several people's,
/// each with a seat of their own.
pub fn personal(kind: Option<&str>) -> bool {
    matches!(kind, Some("claude_max" | "claude_pro"))
}

/// The account of `kept` that `a` is a copy of (GLOSSARY.md "Identity"):
/// for a bound login, the first other bound one of its identity, of any
/// engine (pi and codex on one ChatGPT account); for a
/// portable token, the bound login whose personal organization it names.
/// Two tokens of one organization are no copies: it may be a team's.
pub fn copy_of<'a>(a: &Account, kept: &'a [Account]) -> Option<&'a str> {
    let id = a.identity.as_deref()?;
    let of = |b: &&Account| {
        let bound = b.machine.is_some() && b.name != a.name;
        bound
            && match a.machine {
                Some(_) => b.identity.as_deref() == Some(id) && b.name < a.name,
                None => {
                    b.organization.as_deref() == Some(id)
                        && personal(b.organization_type.as_deref())
                }
            }
    };
    kept.iter().find(of).map(|b| b.name.as_str())
}

/// An account as every command answers it: its terms, and its login
/// (`portable` or `bound`) said outright.
#[derive(Serialize)]
pub struct Shown<'a> {
    #[serde(flatten)]
    account: &'a Account,
    login: &'static str,
    /// The kept account it is a copy of ([`copy_of`]).
    copy_of: Option<&'a str>,
}

impl Account {
    /// It as answered beside the accounts `kept`.
    pub fn shown<'a>(&'a self, kept: &'a [Account]) -> Shown<'a> {
        Shown {
            account: self,
            login: self.login(),
            copy_of: copy_of(self, kept),
        }
    }

    /// Its login's kind: `bound` to a machine, or `portable`.
    pub fn login(&self) -> &'static str {
        if self.machine.is_some() {
            "bound"
        } else {
            "portable"
        }
    }
}

/// An account's terms as `add` and `edit` take them, each `Some` to set it.
#[derive(Debug, Default, clap::Args)]
pub struct Terms {
    /// The machine its bound login lives on; without it the login is portable.
    #[arg(long)]
    pub machine: Option<String>,
    /// When its portable token expires, YYYY-MM-DD (a portable login only).
    #[arg(long)]
    pub expires: Option<String>,
    /// Its bound login's home on --machine (a bound login only).
    #[arg(long)]
    pub home: Option<String>,
    /// The command that prints its portable token, the token its first line (a portable login only): `mytokens get agent-account-claude-felix --field CLAUDE_CODE_OAUTH_TOKEN`. Run by `sh -c` with no input, each time a token is needed.
    #[arg(long)]
    pub token_command: Option<String>,
}

/// What the person asked to change of an account (`add` gives all of it,
/// `edit` some); `unset` names the terms to clear.
#[derive(Debug, Default)]
pub struct Change {
    pub engine: Option<Engine>,
    pub terms: Terms,
    pub unset: Vec<Field>,
}

/// A term `edit --unset` clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Field {
    Machine,
    Expires,
    Home,
    TokenCommand,
}

/// A new account `name` of `engine` with `change`'s terms, or every fault at once.
pub fn new(name: &str, engine: Engine, change: Change) -> Result<Account, Vec<String>> {
    let blank = Account {
        name: name.into(),
        engine,
        machine: None,
        expires: None,
        home: None,
        off: None,
        token_command: None,
        identity: None,
        organization: None,
        organization_type: None,
    };
    let named = check_name(name);
    match (apply(&blank, change), named) {
        (Ok(a), Ok(())) => Ok(a),
        (Ok(_), Err(f)) => Err(vec![f]),
        (Err(faults), Ok(())) => Err(faults),
        (Err(faults), Err(f)) => Err([vec![f], faults].concat()),
    }
}

/// `account` with `change` applied, or every fault at once; `account` is
/// left as it was either way.
pub fn apply(account: &Account, change: Change) -> Result<Account, Vec<String>> {
    let mut faults = Vec::new();
    let mut next = account.clone();
    if let Some(engine) = change.engine {
        next.engine = engine;
    }
    if let Some(machine) = change.terms.machine {
        if machine.trim().is_empty() {
            faults.push(
                "--machine is empty: name the host, or `--unset machine` for a portable login"
                    .into(),
            );
        }
        next.machine = Some(machine);
    }
    if let Some(expires) = change.terms.expires {
        if let Err(f) = check_date(&expires) {
            faults.push(f);
        }
        next.expires = Some(expires);
    }
    if let Some(home) = change.terms.home {
        if home.trim().is_empty() {
            faults.push("--home is empty: give the bound login's home directory".into());
        }
        next.home = Some(home);
    }
    if let Some(command) = change.terms.token_command {
        if command.trim().is_empty() {
            faults.push("--token-command is empty: give the command that prints the token (`mytokens get <entry> --field <field>`)".into());
        }
        next.token_command = Some(command);
    }
    for field in change.unset {
        match field {
            Field::Machine => next.machine = None,
            Field::Expires => next.expires = None,
            Field::Home => next.home = None,
            Field::TokenCommand => next.token_command = None,
        }
    }
    if next.machine.is_some() && next.expires.is_some() {
        faults.push("a bound login (--machine) is refreshed by its engine and has no --expires: drop --expires (`--unset expires`), or drop --machine for a portable token".into());
    }
    if next.machine.is_some() && next.token_command.is_some() {
        faults.push("a bound login (--machine) is its engine's login files in its home and has no --token-command: drop --token-command (`--unset token-command`), or drop --machine for a portable token".into());
    }
    if next.machine.is_none() && next.home.is_some() {
        faults.push("--home is a bound login's home and needs --machine: give the machine it is on, or drop --home (`--unset home`)".into());
    }
    if faults.is_empty() {
        Ok(next)
    } else {
        Err(faults)
    }
}

/// A name as the person gives it: letters, digits, `-`, `_`, `.`, so it is
/// one word on any command line.
fn check_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "account name `{name}` is not one word: use letters, digits, `-`, `_` or `.` (claude-felix)"
        ))
    }
}

/// A calendar date, `YYYY-MM-DD`.
pub fn check_date(s: &str) -> Result<(), String> {
    day_of(s)
        .map(|_| ())
        .ok_or_else(|| format!("`{s}` is not a date: give --expires as YYYY-MM-DD (2027-01-31)"))
}

/// A calendar date `YYYY-MM-DD` as days since 1970-01-01, if it is one.
fn day_of(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        s.get(r)
            .filter(|p| p.bytes().all(|c| c.is_ascii_digit()))?
            .parse()
            .ok()
    };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=days).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    Some(era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468)
}

/// How many days before a portable token expires it is warned of.
pub const WARN_DAYS: i64 = 30;

/// Whether an account is logged in, and for a portable token with an
/// expiry, the days it has left and a warning from `WARN_DAYS` ahead.
#[derive(Debug, PartialEq, Serialize)]
pub struct Status {
    pub logged_in: bool,
    pub days_left: Option<i64>,
    pub warning: Option<String>,
}

/// `account`'s status on day `today` (days since 1970-01-01), `logged_in` as
/// [`crate::login::logged_in`] says.
pub fn status(account: &Account, logged_in: bool, today: i64) -> Status {
    let expires = account.expires.as_deref();
    let days_left = expires.and_then(day_of).map(|d| d - today);
    let renew = format!(
        "make a new token, keep it where its --token-command reads it, and `stablehand account edit {} --expires <its date>`",
        account.name
    );
    let warning = match days_left {
        Some(n) if n < 0 => Some(format!(
            "its token expired on {}: {renew}",
            expires.unwrap_or_default()
        )),
        Some(n) if n <= WARN_DAYS => Some(format!("its token expires in {n} days: {renew}")),
        _ => None,
    };
    Status {
        logged_in,
        days_left,
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_is_a_real_calendar_day() {
        for good in [
            "2027-01-31",
            "2028-02-29",
            "2000-02-29",
            "2026-12-01",
            "2027-04-30",
            "2026-11-30",
        ] {
            assert_eq!(check_date(good), Ok(()), "{good}");
        }
        for bad in [
            "2027-02-29",
            "1900-02-29",
            "2027-13-01",
            "2027-00-10",
            "2027-04-31",
            "2027-01-00",
            "2027-1-31",
            "27-01-31x",
            "2027/01/31",
            "2027-01/31",
            "2027/01-31",
            "2027-+1-31",
            "",
        ] {
            let fault = check_date(bad).unwrap_err();
            assert!(fault.contains("YYYY-MM-DD"), "{bad}: {fault}");
        }
    }

    #[test]
    fn status_counts_the_days_left_and_warns_30_ahead() {
        let today = day_of("2026-11-24").unwrap();
        assert_eq!(today, 1_795_516_200 / 86_400);
        let mut a = new("c", Engine::ClaudeCode, Change::default()).unwrap();
        let none = Status {
            logged_in: true,
            days_left: None,
            warning: None,
        };
        assert_eq!(status(&a, true, today), none);
        for (expires, left, warned) in [
            ("2026-12-25", 31, None),
            ("2026-12-24", 30, Some("expires in 30 days")),
            ("2026-11-24", 0, Some("expires in 0 days")),
            ("2026-11-23", -1, Some("expired on 2026-11-23")),
            ("2028-03-01", 463, None),
        ] {
            a.expires = Some(expires.into());
            let s = status(&a, false, today);
            assert_eq!((s.logged_in, s.days_left), (false, Some(left)), "{expires}");
            match (warned, s.warning) {
                (None, None) => {}
                (Some(part), Some(w)) => {
                    assert!(
                        w.contains(part) && w.contains("stablehand account edit c --expires"),
                        "{w}"
                    )
                }
                (want, got) => panic!("{expires}: {want:?} {got:?}"),
            }
        }
        assert_eq!(day_of("1970-01-01"), Some(0));
        assert_eq!(day_of("2000-03-01"), Some(11_017));
    }

    #[test]
    fn an_account_whose_identity_is_another_s_names_it() {
        let bound = |name: &str, identity: &str| Account {
            machine: Some("mini".into()),
            identity: Some(identity.into()),
            ..new(name, Engine::ClaudeCode, Change::default()).unwrap()
        };
        let token = |name: &str, identity: &str| Account {
            machine: None,
            ..bound(name, identity)
        };
        let max = Account {
            organization: Some("org-max".into()),
            organization_type: Some("claude_max".into()),
            ..bound("claude-code-2", "login-1")
        };
        let team = Account {
            organization: Some("org-team".into()),
            organization_type: Some("claude_team".into()),
            ..bound("work", "login-2")
        };
        let kept = [
            token("claude-code", "org-max"),
            max,
            token("theplant", "org-team"),
            token("theplant-2", "org-team"),
            bound("work-copy", "login-2"),
            team,
            token("alone", "org-other"),
            Account {
                engine: Engine::Codex,
                ..bound("codex", "chatgpt")
            },
            Account {
                engine: Engine::Pi,
                ..bound("pi", "chatgpt")
            },
            // A token is never the account a login is a copy of.
            token("a-token", "login-3"),
            bound("z-login", "login-3"),
        ];
        let of: Vec<_> = kept.iter().map(|a| copy_of(a, &kept)).collect();
        assert_eq!(
            of,
            [
                Some("claude-code-2"),
                None,
                None,
                None,
                Some("work"),
                None,
                None,
                None,
                Some("codex"),
                None,
                None,
            ]
        );
        assert!(personal(Some("claude_pro")) && !personal(Some("claude_team")) && !personal(None));
    }
}
