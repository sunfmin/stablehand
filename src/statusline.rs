//! `statusline`: Claude Code's status line command as a reader. It books
//! the `rate_limits` Claude Code gives on stdin as a reading of one
//! account, and prints the status bar's line, or hands the same stdin to
//! the person's own status line command and lets it print. Local only: no
//! network, so it is as fast as the bar needs.
use crate::account::Engine;
use crate::books::{self, Entry, What, Window};
use crate::readers::claude_code;
use crate::store::Store;
use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};

/// Reads stdin, books its windows as a reading of `account`, and prints
/// the line: `then`'s, run by `sh -c` on the same stdin, where given,
/// exiting as it does. A fault in booking never takes the line away: the
/// line says it, or with `then` stderr does.
pub fn run(account: &str, then: Option<&str>) -> ExitCode {
    let mut stdin = Vec::new();
    let read = std::io::stdin().read_to_end(&mut stdin);
    let windows = claude_code::statusline(&stdin);
    let Some(then) = then else {
        let line = match read
            .map_err(|e| e.to_string())
            .and_then(|_| book(account, &windows))
        {
            Ok(()) => line(account, &windows),
            Err(fault) => format!("stablehand: {fault}"),
        };
        println!("{}", line.replace('\n', " "));
        return ExitCode::SUCCESS;
    };
    // The person's line starts first, so that booking adds nothing to its time.
    let child = Command::new("sh")
        .args(["-c", then])
        .stdin(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            println!("stablehand: --then `{then}` did not start: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut pipe = child.stdin.take();
    let feed = std::thread::spawn(move || {
        // A line that reads none of its stdin closes the pipe: no fault.
        let _ = pipe.as_mut().map(|p| p.write_all(&stdin));
    });
    if let Err(fault) = book(account, &windows) {
        eprintln!("stablehand: {fault}");
    }
    let _ = feed.join();
    match child.wait() {
        Ok(status) => ExitCode::from(status.code().map_or(1, |c| c as u8)),
        Err(e) => {
            eprintln!("stablehand: --then `{then}`: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Books `windows` as a reading of `name` now, where they are news.
fn book(name: &str, windows: &[Window]) -> Result<(), String> {
    let store = Store::open()?;
    let a = crate::kept(&store, name).map_err(|r| r.text)?;
    if a.engine != Engine::ClaudeCode {
        return Err(format!(
            "`{name}` is a {} account: the status line books claude-code's; name a claude-code account with --account",
            a.engine.name()
        ));
    }
    if windows.is_empty() || !books::is_news(&store.entries(name)?, windows) {
        return Ok(());
    }
    let at = crate::now()?;
    let what = What::Read(windows.to_vec());
    store.book(name, &Entry { at, what })
}

/// The bar's line: the account, and each window's use, rounded.
fn line(account: &str, windows: &[Window]) -> String {
    let mut parts = vec![account.to_string()];
    for w in windows {
        let name = match w.name.as_str() {
            "five_hour" => "5h",
            "seven_day" => "7d",
            other => other,
        };
        parts.push(format!("{name} {:.0}%", w.used_percent.round()));
    }
    parts.join(" · ")
}
