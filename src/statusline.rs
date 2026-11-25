//! `statusline`: Claude Code's status line command as a reader. It books
//! the `rate_limits` Claude Code gives on stdin as a reading of one
//! account, and prints the status bar's line, or hands the same stdin to
//! the person's own status line command and lets it print. Local only: no
//! network, so it is as fast as the bar needs.
use crate::account::{Account, Engine};
use crate::books::{self, Entry, What, Window};
use crate::readers::claude_code;
use crate::store::Store;
use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};

/// The variable `stablehand run` names its child's account in, and the
/// status line reads where no `--account` is given: a name, never a value.
pub const ACCOUNT_VAR: &str = "STABLEHAND_ACCOUNT";

/// A reading older than this many seconds is shown with its age.
const FRESH_SECS: u64 = 10 * 60;

/// Reads stdin, books its windows as a reading of `account` (else
/// [`ACCOUNT_VAR`]'s), and prints the line: `then`'s, run by `sh -c` on the
/// same stdin, where given, exiting as it does. With no account, nothing
/// is booked and stablehand's line is empty. A fault in booking never
/// takes the line away: the line says it, or with `then` stderr does.
pub fn run(account: Option<String>, then: Option<&str>) -> ExitCode {
    let account = account.or_else(|| std::env::var(ACCOUNT_VAR).ok());
    let account = account.filter(|a| !a.is_empty());
    // What could not be read is as good as not given: nothing to book.
    let mut stdin = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut stdin);
    let windows = claude_code::statusline(&stdin);
    let Some(then) = then else {
        let line = match &account {
            None => String::new(),
            Some(account) => match shown(account, &windows) {
                Ok(shown) => line(account, &shown),
                Err(fault) => format!("stablehand: {fault}"),
            },
        };
        println!("{}", line.replace('\n', " "));
        return ExitCode::SUCCESS;
    };
    // The person's line starts first, so that booking adds nothing to its
    // time. Its exit is the answer's; 127, as sh's for a command not
    // found, where /bin/sh could not be run.
    let status = Command::new("/bin/sh")
        .args(["-c", then])
        .stdin(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            let mut pipe = child.stdin.take();
            // A line that reads none of its stdin closes the pipe: no fault.
            let feed = std::thread::spawn(move || pipe.as_mut().map(|p| p.write_all(&stdin)));
            if let Some(Err(fault)) = account.map(|a| book(&a, &windows)) {
                eprintln!("stablehand: {fault}");
            }
            let _ = feed.join();
            child.wait()
        });
    ExitCode::from(status.ok().and_then(|s| s.code()).unwrap_or(127) as u8)
}

/// One window as the bar shows it: its use (none once it has reset since
/// it was read), and its reading's age where that is past [`FRESH_SECS`].
#[derive(Debug, PartialEq)]
struct Shown {
    name: String,
    used: Option<f64>,
    age: Option<u64>,
}

/// Books `windows` of `name`, and answers what the bar shows: `windows`,
/// or with none given, the books' latest as `stablehand status` has them.
fn shown(name: &str, windows: &[Window]) -> Result<Vec<Shown>, String> {
    let (store, a) = book(name, windows)?;
    if !windows.is_empty() {
        let given = windows.iter().map(|w| Shown {
            name: w.name.clone(),
            used: Some(w.used_percent),
            age: None,
        });
        return Ok(given.collect());
    }
    let now = crate::now()?;
    let status = books::status(&crate::usage::book_of(&store, &a)?, now);
    let booked = status.windows.into_iter().map(|w| Shown {
        age: Some(now.saturating_sub(w.at)).filter(|&age| age > FRESH_SECS),
        name: w.name,
        used: w.used_percent,
    });
    Ok(booked.collect())
}

/// Books `windows` as a reading of `name` now, where they are news;
/// answers the store and the account.
fn book(name: &str, windows: &[Window]) -> Result<(Store, Account), String> {
    let store = Store::open()?;
    let a = crate::kept(&store, name).map_err(|r| r.text)?;
    if a.engine != Engine::ClaudeCode {
        return Err(format!(
            "`{name}` is a {} account: the status line books claude-code's; name a claude-code account with --account",
            a.engine.name()
        ));
    }
    if windows.is_empty() || !books::is_news(&store.entries(name)?, windows) {
        return Ok((store, a));
    }
    let at = crate::now()?;
    let what = What::Read(windows.to_vec());
    store.book(name, &Entry { at, what })?;
    Ok((store, a))
}

/// The bar's line: the account, and each window's use, rounded, with its
/// age where it is shown one.
fn line(account: &str, windows: &[Shown]) -> String {
    let mut parts = vec![account.to_string()];
    for w in windows {
        let name = match w.name.as_str() {
            "five_hour" => "5h",
            "seven_day" => "7d",
            other => other,
        };
        let used = w
            .used
            .map_or("reset".to_string(), |u| format!("{:.0}%", u.round()));
        let age = w
            .age
            .map_or(String::new(), |a| format!(" ({} ago)", age(a)));
        parts.push(format!("{name} {used}{age}"));
    }
    parts.join(" · ")
}

/// `secs` in its largest whole unit: `25m`, `3h`, `2d`.
fn age(secs: u64) -> String {
    match secs {
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 2 * 86_400 => format!("{}h", s / 3_600),
        s => format!("{}d", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(name: &str, used: Option<f64>, age: Option<u64>) -> Shown {
        Shown {
            name: name.into(),
            used,
            age,
        }
    }

    #[test]
    fn the_line_names_each_window_short_where_it_knows_it() {
        let windows = [
            w("five_hour", Some(23.5), None),
            w("seven_day", Some(41.2), None),
            w("seven_day_opus", Some(9.0), None),
        ];
        assert_eq!(
            line("a", &windows),
            "a · 5h 24% · 7d 41% · seven_day_opus 9%"
        );
        assert_eq!(line("a", &[]), "a");
    }

    #[test]
    fn a_booked_window_shows_its_age_past_ten_minutes_and_a_reset_one_no_use() {
        let windows = [
            w("five_hour", None, Some(3 * 3_600 + 59)),
            w("seven_day", Some(70.0), Some(25 * 60)),
            w("seven_day_opus", Some(1.0), Some(3 * 86_400)),
        ];
        assert_eq!(
            line("a", &windows),
            "a · 5h reset (3h ago) · 7d 70% (25m ago) · seven_day_opus 1% (3d ago)"
        );
        assert_eq!(age(FRESH_SECS + 1), "10m");
        assert_eq!(age(2 * 86_400 - 1), "47h");
    }
}
