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
    // What could not be read is as good as not given: nothing to book.
    let mut stdin = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut stdin);
    let windows = claude_code::statusline(&stdin);
    let Some(then) = then else {
        let line = match book(account, &windows) {
            Ok(()) => line(account, &windows),
            Err(fault) => format!("stablehand: {fault}"),
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
            if let Err(fault) = book(account, &windows) {
                eprintln!("stablehand: {fault}");
            }
            let _ = feed.join();
            child.wait()
        });
    ExitCode::from(status.ok().and_then(|s| s.code()).unwrap_or(127) as u8)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_each_window_short_where_it_knows_it() {
        let w = |name: &str, used| Window {
            name: name.into(),
            group: None,
            models: Vec::new(),
            used_percent: used,
            resets_at: None,
            minutes: None,
        };
        let windows = [
            w("five_hour", 23.5),
            w("seven_day", 41.2),
            w("seven_day_opus", 9.0),
        ];
        assert_eq!(
            line("a", &windows),
            "a · 5h 24% · 7d 41% · seven_day_opus 9%"
        );
        assert_eq!(line("a", &[]), "a");
    }
}
