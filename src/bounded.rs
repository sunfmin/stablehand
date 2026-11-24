//! A program run for its exit and stdout within a time bound: past it, it is
//! killed with every process it started (it leads a process group of its
//! own), so nothing it began outlives the bound. agy's reader and a
//! portable account's token command run through it.
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
        let ten = Duration::from_secs(10);
        assert_eq!(
            run(sh("echo hi; exit 3"), ten).unwrap(),
            Ran::Exited(Some(3), b"hi\n".to_vec())
        );
        assert!(run(Command::new("/no/such/program"), ten).is_err());
    }
}
