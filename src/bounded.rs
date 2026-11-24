//! A program run for its exit and stdout within a time bound: past it, it is
//! killed with every process it started (it leads a process group of its
//! own), so nothing it began outlives the bound. agy's reader, claude-code's
//! question and a portable account's token command run through it.
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// What came of a bounded run.
#[derive(Debug, PartialEq)]
pub enum Ran {
    /// It exited (its code; none if a signal ended it) with its stdout.
    Exited(Option<i32>, Vec<u8>),
    /// It, or what it started holding its stdout, ran past the bound.
    RanPast,
}

/// Runs `cmd` with its stdout piped, its stdin and stderr as `cmd` sets
/// them, for at most `limit`; `Err` when it could not start.
pub fn run(mut cmd: Command, limit: Duration) -> std::io::Result<Ran> {
    let mut child = cmd.stdout(Stdio::piped()).process_group(0).spawn()?;
    let mut out = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut printed = Vec::new();
        let _ = out.read_to_end(&mut printed);
        let _ = tx.send(printed);
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            stop(&mut child);
            return Ok(Ran::RanPast);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // What it started may hold its stdout open past its own exit.
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(printed) => Ok(Ran::Exited(status.code(), printed)),
        Err(_) => {
            stop(&mut child);
            Ok(Ran::RanPast)
        }
    }
}

/// What a reader's program answered, `cmd` run as [`run`] runs it: its exit
/// code and stdout; or, as a reader's failure says it, that `what` (its
/// line, as a fault names it) did not start (`install` the fix) or answered
/// nothing within `limit`.
pub fn answer(
    cmd: Command,
    limit: Duration,
    what: &str,
    install: &str,
) -> Result<(Option<i32>, String), String> {
    match run(cmd, limit) {
        Err(e) => Err(format!("{what}: {e}: {install}")),
        Ok(Ran::RanPast) => Err(format!(
            "{what} answered nothing in {}s: try again later (`stablehand read --now`)",
            limit.as_secs()
        )),
        Ok(Ran::Exited(code, out)) => Ok((code, String::from_utf8_lossy(&out).into_owned())),
    }
}

/// Kills `child`'s process group, and `child` itself.
fn stop(child: &mut Child) {
    let group = format!("-{}", child.id());
    let _ = Command::new("kill")
        .args(["-KILL", "--", &group])
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        cmd
    }

    #[test]
    fn a_program_past_its_bound_is_killed_with_what_it_started() {
        let started = Instant::now();
        let limit = Duration::from_millis(300);
        assert_eq!(run(sh("sleep 30"), limit).unwrap(), Ran::RanPast);
        // A child left holding stdout after its parent exits is past it too.
        assert_eq!(run(sh("sleep 30 & echo x"), limit).unwrap(), Ran::RanPast);
        assert!(started.elapsed() < Duration::from_secs(5));
        // Stopped means gone: it, and what it started, are killed.
        let dir = std::env::temp_dir().join(format!("stablehand-bounded-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (own, started_by) = (dir.join("own"), dir.join("child"));
        let script = format!(
            "echo $$ > {}; sleep 30 & echo $! > {}; wait",
            own.display(),
            started_by.display()
        );
        assert_eq!(run(sh(&script), limit).unwrap(), Ran::RanPast);
        for pid in [&own, &started_by] {
            let pid = std::fs::read_to_string(pid).unwrap();
            assert!(gone(pid.trim()), "{pid} outlived the bound");
        }
        let ten = Duration::from_secs(10);
        assert_eq!(
            run(sh("echo hi; exit 3"), ten).unwrap(),
            Ran::Exited(Some(3), b"hi\n".to_vec())
        );
        assert!(run(Command::new("/no/such/program"), ten).is_err());
    }

    #[test]
    fn a_reader_s_program_answers_or_says_why_not() {
        let ms = Duration::from_millis(300);
        let ten = Duration::from_secs(10);
        assert_eq!(
            answer(sh("echo hi"), ten, "`x`", "fix"),
            Ok((Some(0), "hi\n".into()))
        );
        let fault = answer(Command::new("/no/such/program"), ten, "`x`", "install x").unwrap_err();
        assert!(
            fault.starts_with("`x`: ") && fault.ends_with(": install x"),
            "{fault}"
        );
        assert_eq!(
            answer(sh("sleep 30"), ms, "`x`", "fix"),
            Err("`x` answered nothing in 0s: try again later (`stablehand read --now`)".into())
        );
    }

    /// Whether no process `pid` is left, within a second of asking.
    fn gone(pid: &str) -> bool {
        (0..100).any(|_| {
            let alive = Command::new("kill")
                .args(["-0", pid])
                .stderr(Stdio::null())
                .status();
            let dead = !alive.unwrap().success();
            if !dead {
                std::thread::sleep(Duration::from_millis(10));
            }
            dead
        })
    }
}
