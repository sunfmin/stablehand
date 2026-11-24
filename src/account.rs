//! What an account is and which of its terms may be kept: the register's
//! decisions, with no I/O. The store keeps what these functions accept; the
//! command line hands them what the person typed.
use serde::Serialize;

/// An agent program a session runs in (GLOSSARY.md "Engine"). The one list of
/// them: `--engine`'s values, its help and its refusal are derived from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
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
    /// Sessions at once when the person says none: claude-code's accounts
    /// run 2, as Guild's ledger has it; the others are not limited here.
    pub fn default_sessions(self) -> Option<u32> {
        (self == Engine::ClaudeCode).then_some(2)
    }

    /// Its name, as `--engine` takes it.
    pub fn name(self) -> String {
        let value = clap::ValueEnum::to_possible_value(&self).expect("every engine is a value");
        value.get_name().to_string()
    }

    /// How the engine logs in, its own way (docs/adr/0001 "Logins are the
    /// engine's own"): the one table of each engine's login facts.
    pub fn login(self) -> Login {
        use Engine::*;
        let (home_var, bound, token, usage_host): (_, &[&str], Option<&[&str]>, _) = match self {
            ClaudeCode => (
                "CLAUDE_CONFIG_DIR",
                &["claude", "auth", "login"],
                Some(&["claude", "setup-token"]),
                "api.anthropic.com",
            ),
            Codex => ("CODEX_HOME", &["codex", "login"], None, "chatgpt.com"),
            Pi => ("PI_CODING_AGENT_DIR", &["pi"], None, "chatgpt.com"),
            Kimi => ("KIMI_CODE_HOME", &["kimi", "login"], None, "api.kimi.com"),
            Agy => ("HOME", &["agy"], None, "cloudcode-pa.googleapis.com"),
            Grok => (
                "GROK_HOME",
                &["grok", "login"],
                None,
                "cli-chat-proxy.grok.com",
            ),
            Dsh => ("DSH_HOME", &["dsh", "tui"], None, "platform.deepseek.com"),
        };
        Login {
            home_var,
            bound,
            token,
            // The others read a token from where their own login put it, as
            // far as stablehand knows: their sessions start bound.
            token_var: (self == ClaudeCode).then_some("CLAUDE_CODE_OAUTH_TOKEN"),
            usage_host,
        }
    }
}

/// An engine's login facts. agy, pi and dsh have no login command: started
/// bare in their home, they sign in on first use.
pub struct Login {
    /// The variable the engine reads for its home.
    pub home_var: &'static str,
    /// The command that logs a bound account in, run in its home.
    pub bound: &'static [&'static str],
    /// The command that prints a long-lived token, where the engine has one.
    pub token: Option<&'static [&'static str]>,
    /// The variable a session reads a portable token from, where the engine has one.
    pub token_var: Option<&'static str>,
    /// The host its usage is read from: the one host sesame sends its token to.
    pub usage_host: &'static str,
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
    /// Sessions at once; none: no limit.
    pub sessions: Option<u32>,
    /// When its portable token expires, `YYYY-MM-DD`.
    pub expires: Option<String>,
    /// Its bound login's home on `machine`.
    pub home: Option<String>,
    pub off: Option<Off>,
    /// When its bound login last logged in, in `home` on `machine` (UTC,
    /// RFC 3339); a portable login's token is sesame's to know.
    pub logged_in_at: Option<String>,
}

/// An account as every command answers it: its terms, and its login
/// (`portable` or `bound`) said outright.
#[derive(Serialize)]
pub struct Shown<'a> {
    #[serde(flatten)]
    account: &'a Account,
    login: &'static str,
}

impl Account {
    pub fn shown(&self) -> Shown<'_> {
        Shown {
            account: self,
            login: self.login(),
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
    /// Sessions it runs at once (`add` without it: claude-code 2, other engines no limit).
    #[arg(long)]
    pub sessions: Option<u32>,
    /// When its portable token expires, YYYY-MM-DD (a portable login only).
    #[arg(long)]
    pub expires: Option<String>,
    /// Its bound login's home on --machine (a bound login only).
    #[arg(long)]
    pub home: Option<String>,
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
    Sessions,
    Expires,
    Home,
}

/// A new account `name` of `engine` with `change`'s terms, or every fault at once.
pub fn new(name: &str, engine: Engine, change: Change) -> Result<Account, Vec<String>> {
    let blank = Account {
        name: name.into(),
        engine,
        machine: None,
        sessions: engine.default_sessions(),
        expires: None,
        home: None,
        off: None,
        logged_in_at: None,
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
    if let Some(sessions) = change.terms.sessions {
        if sessions == 0 {
            faults.push("--sessions 0 would start nothing: give 1 or more, or switch the account off with `stablehand off`".into());
        }
        next.sessions = Some(sessions);
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
    for field in change.unset {
        match field {
            Field::Machine => next.machine = None,
            Field::Sessions => next.sessions = None,
            Field::Expires => next.expires = None,
            Field::Home => next.home = None,
        }
    }
    // A login is of one engine in one home: moving either forgets it.
    if (next.engine, &next.machine, &next.home) != (account.engine, &account.machine, &account.home)
    {
        next.logged_in_at = None;
    }
    if next.machine.is_some() && next.expires.is_some() {
        faults.push("a bound login (--machine) is refreshed by its engine and has no --expires: drop --expires (`--unset expires`), or drop --machine for a portable token".into());
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
/// its login's holder says: the register for a bound login, sesame for a
/// portable one.
pub fn status(account: &Account, logged_in: bool, today: i64) -> Status {
    let expires = account.expires.as_deref();
    let days_left = expires.and_then(day_of).map(|d| d - today);
    let renew = format!(
        "make a new token and run `stablehand account login {} --expires <its date>`",
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
                        w.contains(part) && w.contains("stablehand account login c"),
                        "{w}"
                    )
                }
                (want, got) => panic!("{expires}: {want:?} {got:?}"),
            }
        }
        assert_eq!(day_of("1970-01-01"), Some(0));
        assert_eq!(day_of("2000-03-01"), Some(11_017));
    }
}
