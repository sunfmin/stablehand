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
#[derive(Debug, Clone, PartialEq)]
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
}

/// An account as every command answers it: its terms, and its login
/// (`portable` or `bound`) said outright.
#[derive(Serialize)]
pub struct Shown<'a> {
    name: &'a str,
    engine: Engine,
    login: &'static str,
    machine: Option<&'a str>,
    sessions: Option<u32>,
    expires: Option<&'a str>,
    home: Option<&'a str>,
    off: Option<&'a Off>,
}

impl Account {
    pub fn shown(&self) -> Shown<'_> {
        Shown {
            name: &self.name,
            engine: self.engine,
            login: if self.machine.is_some() {
                "bound"
            } else {
                "portable"
            },
            machine: self.machine.as_deref(),
            sessions: self.sessions,
            expires: self.expires.as_deref(),
            home: self.home.as_deref(),
            off: self.off.as_ref(),
        }
    }
}

/// What the person asked to change of an account (`add` gives all of it,
/// `edit` some): each field `Some` to set it; `unset` names those to clear.
#[derive(Debug, Default)]
pub struct Change {
    pub engine: Option<Engine>,
    pub machine: Option<String>,
    pub sessions: Option<u32>,
    pub expires: Option<String>,
    pub home: Option<String>,
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
    if let Some(machine) = change.machine {
        if machine.trim().is_empty() {
            faults.push(
                "--machine is empty: name the host, or `--unset machine` for a portable login"
                    .into(),
            );
        }
        next.machine = Some(machine);
    }
    if let Some(sessions) = change.sessions {
        if sessions == 0 {
            faults.push("--sessions 0 would start nothing: give 1 or more, or switch the account off with `stablehand off`".into());
        }
        next.sessions = Some(sessions);
    }
    if let Some(expires) = change.expires {
        if let Err(f) = check_date(&expires) {
            faults.push(f);
        }
        next.expires = Some(expires);
    }
    if let Some(home) = change.home {
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
    let fault = || format!("`{s}` is not a date: give --expires as YYYY-MM-DD (2027-01-31)");
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return Err(fault());
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        s.get(r)
            .filter(|p| p.bytes().all(|c| c.is_ascii_digit()))?
            .parse()
            .ok()
    };
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return Err(fault());
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(fault()),
    };
    if (1..=days).contains(&d) {
        Ok(())
    } else {
        Err(fault())
    }
}

/// `secs` after the Unix epoch as UTC RFC 3339 (`2026-11-24T08:30:00Z`).
pub fn utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil_from_days, for days since 1970-01-01.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
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
            "2027-+1-31",
            "",
        ] {
            let fault = check_date(bad).unwrap_err();
            assert!(fault.contains("YYYY-MM-DD"), "{bad}: {fault}");
        }
    }

    #[test]
    fn utc_says_the_calendar_time() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_825_599), "2000-02-29T11:59:59Z");
        assert_eq!(utc(1_795_516_200), "2026-11-24T10:30:00Z");
        assert_eq!(utc(4_107_542_399), "2100-02-28T23:59:59Z");
    }
}
