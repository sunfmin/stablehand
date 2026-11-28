//! `statusline`: Claude Code's status line command as a reader. It books
//! the `rate_limits` Claude Code gives on stdin as a reading of one
//! account, and prints the status bar's line, or hands the same stdin to
//! the person's own status line command and lets it print. Local only: no
//! network, so it is as fast as the bar needs. Where Claude Code gives no
//! windows (a `claude setup-token` session's never does), it starts
//! `stablehand read` detached to keep the books fresh, and does not wait.
use crate::account::{Account, Engine};
use crate::books::{self, Entry, What, Window};
use crate::readers::claude_code;
use crate::store::Store;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// The variable `stablehand run` names its child's account in, and the
/// status line reads where no `--account` is given: a name, never a value.
pub const ACCOUNT_VAR: &str = "STABLEHAND_ACCOUNT";

/// A reading older than this many seconds is shown with its age, and,
/// where Claude Code gives none, read again.
const FRESH_SECS: u64 = 10 * 60;

/// Reads stdin, books its windows as a reading of `account` (else
/// [`ACCOUNT_VAR`]'s, else the [`session_s`]), and prints the line:
/// `then`'s, run by `sh -c` on the same stdin with the account in
/// [`ACCOUNT_VAR`], where given, exiting as it does. With no account,
/// nothing is booked and stablehand's line is empty. A fault in booking
/// never takes the line away: the line says it, or with `then` stderr does.
pub fn run(account: Option<String>, then: Option<&str>) -> ExitCode {
    let account = account.or_else(|| std::env::var(ACCOUNT_VAR).ok());
    let account = account.filter(|a| !a.is_empty()).or_else(session_s);
    // What could not be read is as good as not given: nothing to book.
    let mut stdin = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut stdin);
    let windows = claude_code::statusline(&stdin);
    let Some(then) = then else {
        let line = match &account {
            None => String::new(),
            Some(account) => {
                match shown(account, &windows).and_then(|s| Ok(line(account, &s, crate::now()?))) {
                    Ok(line) => line,
                    Err(fault) => format!("stablehand: {fault}"),
                }
            }
        };
        println!("{}", line.replace('\n', " "));
        return ExitCode::SUCCESS;
    };
    // The person's line starts first, so that booking adds nothing to its
    // time. Its exit is the answer's; 127, as sh's for a command not
    // found, where /bin/sh could not be run.
    let mut sh = Command::new("/bin/sh");
    match &account {
        Some(account) => sh.env(ACCOUNT_VAR, account),
        None => sh.env_remove(ACCOUNT_VAR),
    };
    let status = (sh.args(["-c", then]).stdin(Stdio::piped()).spawn()).and_then(|mut child| {
        let mut pipe = child.stdin.take();
        // A line that reads none of its stdin closes the pipe: no fault.
        let feed = std::thread::spawn(move || pipe.as_mut().map(|p| p.write_all(&stdin)));
        if let Some(Err(fault)) = account.map(|a| shown(&a, &windows)) {
            eprintln!("stablehand: {fault}");
        }
        let _ = feed.join();
        child.wait()
    });
    ExitCode::from(status.ok().and_then(|s| s.code()).unwrap_or(127) as u8)
}

/// The account of the session the bar is Claude Code's, from its
/// environment alone (no token command is run on a refresh): none on
/// another endpoint (`ANTHROPIC_BASE_URL`) or a token (it tells no account
/// by itself); otherwise the claude-code account bound to this machine whose
/// home is the session's config dir, where it is not off.
fn session_s() -> Option<String> {
    let set = |var: &str| std::env::var_os(var).filter(|v| !v.is_empty());
    let login = Engine::ClaudeCode.login();
    if set("ANTHROPIC_BASE_URL").is_some() || set(login.token_var?).is_some() {
        return None;
    }
    let dir = match set(login.home_var) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(set("HOME")?).join(login.default_home),
    };
    let here = crate::login::this_machine().ok()?;
    let kept = Store::open().ok()?.list().ok()?.into_iter().find(|a| {
        a.engine == Engine::ClaudeCode
            && a.machine.as_ref() == Some(&here)
            && (a.home.as_ref()).is_some_and(|h| crate::scan::same(Path::new(h), &dir))
    })?;
    kept.off.is_none().then_some(kept.name)
}

/// One window as the bar shows it: its name, its use (none once it has
/// reset since it was read), its reading's age in seconds, where booked,
/// and when it resets, where it says.
type Shown = (String, Option<f64>, Option<u64>, Option<u64>);

/// Books `windows` of `name`, and answers what the bar shows: `windows`,
/// or with none given, the books' latest as `stablehand status` has them,
/// a read of the account started where they are past [`FRESH_SECS`].
fn shown(name: &str, windows: &[Window]) -> Result<Vec<Shown>, String> {
    let (store, a) = book(name, windows)?;
    if !windows.is_empty() {
        let given = windows
            .iter()
            .map(|w| (w.name.clone(), Some(w.used_percent), None, w.resets_at));
        return Ok(given.collect());
    }
    let now = crate::now()?;
    let status = books::status(&crate::usage::book_of(&store, &a)?, now);
    if !status.refused && books::read_after(&status, FRESH_SECS, now).is_none() {
        // Not started is as good as not due: the next refresh tries again.
        start_read(&store, name);
    }
    let booked = (status.windows.into_iter()).map(|w| {
        (
            w.name,
            w.used_percent,
            Some(now.saturating_sub(w.at)),
            w.resets_at,
        )
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

/// Starts `stablehand read --now <name>` detached, in a process group of
/// its own, with no stdin, stdout or stderr, unless a read it started
/// still runs: the read holds [`Store::reading_lock`] locked (it inherits
/// the lock, and the kernel lets it go when the read ends, however it
/// ends). Answers none where none started.
fn start_read(store: &Store, name: &str) -> Option<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(store.reading_lock(name))
        .ok()?;
    let fd = lock.as_raw_fd();
    // SAFETY: flock on a descriptor this process holds open.
    if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return None;
    }
    let mut read = Command::new(std::env::current_exe().ok()?);
    read.args(["read", "--now", name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    // SAFETY: between fork and exec it only calls fcntl, async-signal-safe,
    // so that the read keeps the locked descriptor open.
    unsafe {
        read.pre_exec(move || {
            libc::fcntl(fd, libc::F_SETFD, 0);
            Ok(())
        });
    }
    read.spawn().ok().map(drop)
}

/// The bar's line: the account, and each window's use, rounded, with how
/// long until it resets where it says, and its age where it is past
/// [`FRESH_SECS`], at `now`.
fn line(account: &str, windows: &[Shown], now: u64) -> String {
    let mut parts = vec![account.to_string()];
    for (name, used, age_secs, resets_at) in windows {
        let name = match name.as_str() {
            "five_hour" => "5h",
            "seven_day" => "7d",
            other => other,
        };
        let used = used.map_or("reset".to_string(), |u| format!("{:.0}%", u.round()));
        let age = age_secs
            .filter(|&a| a > FRESH_SECS)
            .map_or(String::new(), |a| format!(" ({} ago)", age(a)));
        let left = crate::time::until(*resets_at, now).map_or(String::new(), |l| {
            format!(" resets in {}", crate::time::span(l))
        });
        parts.push(format!("{name} {used}{left}{age}"));
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
        (name.into(), used, age, None)
    }

    #[test]
    fn the_line_names_each_window_short_where_it_knows_it() {
        let windows = [
            w("five_hour", Some(23.5), None),
            w("seven_day", Some(41.2), None),
            w("seven_day_opus", Some(9.0), None),
        ];
        assert_eq!(
            line("a", &windows, 0),
            "a · 5h 24% · 7d 41% · seven_day_opus 9%"
        );
        assert_eq!(line("a", &[], 0), "a");
    }

    #[test]
    fn a_window_shows_how_long_until_it_resets_where_it_says() {
        const NOW: u64 = 1_800_000_000;
        let windows = [
            (
                "five_hour".into(),
                Some(24.0),
                None,
                Some(NOW + 4 * 3_600 + 12 * 60),
            ),
            (
                "seven_day".into(),
                Some(70.0),
                Some(25 * 60),
                Some(NOW + 6 * 86_400 + 6 * 3_600),
            ),
            ("past".into(), Some(1.0), None, Some(NOW)),
        ];
        assert_eq!(
            line("a", &windows, NOW),
            "a · 5h 24% resets in 4h 12m · 7d 70% resets in 6d 6h (25m ago) · past 1%"
        );
    }

    #[test]
    fn a_booked_window_shows_its_age_past_ten_minutes_and_a_reset_one_no_use() {
        let windows = [
            w("five_hour", None, Some(3 * 3_600 + 59)),
            w("seven_day", Some(70.0), Some(25 * 60)),
            w("seven_day_opus", Some(1.0), Some(3 * 86_400)),
            w("fresh", Some(2.0), Some(600)),
            w("just_past", Some(3.0), Some(601)),
        ];
        assert_eq!(
            line("a", &windows, 0),
            "a · 5h reset (3h ago) · 7d 70% (25m ago) · seven_day_opus 1% (3d ago) · fresh 2% · just_past 3% (10m ago)"
        );
        assert_eq!(age(3_599), "59m");
        assert_eq!(age(3_600), "1h");
        assert_eq!(age(2 * 86_400 - 1), "47h");
        assert_eq!(age(2 * 86_400), "2d");
    }
}
