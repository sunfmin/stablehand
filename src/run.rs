//! `stablehand run`: starts an engine's own command on the account (and
//! model) stablehand picks, handing it the login in its environment alone,
//! and waits for it: the terminal, the signals and the exit status are the
//! child's own. While it runs its session is kept in the register, so a
//! pick counts it (docs/adr/0002).
use crate::account::{Account, Engine};
use crate::command;
use crate::login::{self, Credential, Link};
use crate::refusal::{Code, Refused};
use crate::store::Store;
use crate::usage;
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, ExitCode, ExitStatus};
use std::sync::atomic::{AtomicI32, Ordering};

/// What `run` was asked.
pub struct Asked {
    pub command: Vec<String>,
    pub engine: Option<Engine>,
    pub account: Option<String>,
    pub model: Option<String>,
    pub home: Option<String>,
}

fn bad(text: impl Into<String>) -> Refused {
    Refused::new(Code::BadRun, text)
}

/// Picks, starts and waits; the child's exit status as its own, or the
/// refusal, nothing started: none picked is answered as `pick`'s none is,
/// with its `error` and `code`.
pub fn run(asked: Asked, now: u64) -> Result<ExitCode, Refused> {
    let store = Store::open()?;
    let account = match &asked.account {
        Some(name) => Some(crate::kept(&store, name)?),
        None => None,
    };
    let of_account = account.as_ref().map(|a| (a.name.as_str(), a.engine));
    let engine = command::engine(&asked.command, asked.engine, of_account).map_err(bad)?;
    let in_command = match (engine, asked.command.get(1..)) {
        (Some(e), Some(args)) => command::named_model(e, args),
        _ => None,
    };
    let model = match (asked.model, in_command) {
        (Some(m), Some(c)) if m != c => {
            return Err(bad(format!(
                "--model {m} and the command's model {c} differ: name one"
            )));
        }
        (m, c) => m.or(c),
    };
    if let (Some(e), Some(m)) = (engine, &model)
        && e.model_flags().is_empty()
    {
        return Err(bad(format!(
            "{} takes no model on its command line (its profile names it): drop --model {m}",
            e.name()
        )));
    }
    let accounts = match account {
        Some(a) => vec![a],
        None => store.list()?,
    };
    let ask = usage::Ask {
        engines: engine.into_iter().collect(),
        model,
        running: Vec::new(),
        machine: None,
        stop_percent: crate::pick::STOP_PERCENT,
    };
    let picked = usage::pick_now(&store, &accounts, ask, now)?;
    if picked.get("none").is_some() {
        return Ok(none(picked));
    }
    let engine: Engine =
        serde_json::from_value(picked["engine"].clone()).map_err(|e| e.to_string())?;
    let model = picked["model"].as_str().map(str::to_string);
    let name = picked["account"].as_str().unwrap_or_default();
    let a = crate::kept(&store, name)?;
    let line = command::line(engine, &asked.command, model.as_deref());
    let mut child = Command::new(&line[0]);
    child.args(&line[1..]);
    hand_login(&mut child, &a, asked.home.as_deref())?;
    let said = model.map_or(String::new(), |m| format!(" ({m})"));
    eprintln!("stablehand: {} on {}{said}", engine.name(), a.name);
    forward_signals();
    let mut started = child
        .spawn()
        .map_err(|e| format!("`{}` did not start: {e}: is it on PATH?", line[0]))?;
    let pid = started.id();
    CHILD.store(pid as i32, Ordering::SeqCst);
    if let Err(e) = store.start_session(&a.name, pid, now) {
        eprintln!("stablehand: its session is not counted: {e}");
    }
    let status = started.wait().map_err(|e| e.to_string());
    if let Err(e) = store.end_session(pid) {
        eprintln!(
            "stablehand: its session is still kept (dropped once its pid is found dead): {e}"
        );
    }
    Ok(passed_on(status?))
}

/// Pick's none, as a refusal: its `until` and `why` with the `error` and
/// `code` every refusal has.
fn none(mut picked: Value) -> ExitCode {
    let code = Code::NonePicked;
    let until = picked["until"]
        .as_str()
        .map_or("none says when".into(), |u| {
            format!("the earliest lifts at {u}")
        });
    picked["error"] = json!(format!(
        "no account takes one more session now ({until}): `why` says why each was passed over; start nothing, or switch one on, log one in, or wait"
    ));
    picked["code"] = json!(agent_help::Table::entry(&code).name);
    println!("{picked}");
    ExitCode::from(code.exit())
}

/// Sets `child`'s environment to `a`'s login: a portable token in its
/// engine's token variable; a bound home in its engine's home variable,
/// the token variable cleared so one in stablehand's own environment does
/// not stand in for it. With `home`, the home variable is `home`, a bound
/// login's files linked into it.
fn hand_login(child: &mut Command, a: &Account, home: Option<&str>) -> Result<(), Refused> {
    let login = a.engine.login();
    match login::credential(a)? {
        Credential::Token { var, token } => {
            child.env(var, token);
            if let Some(home) = home {
                made(home)?;
                child.env(login.home_var, home);
            }
        }
        Credential::Files(links) => {
            if let Some(var) = login.token_var {
                child.env_remove(var);
            }
            let dir = match home {
                Some(_) if a.engine == Engine::ClaudeCode => {
                    return Err(bad(
                        "--home is refused for a bound claude-code account: Claude Code writes a refreshed login as a new file over a linked .credentials.json, leaving two copies of one refresh token that log each other out (docs/adr/0002); run on its own home, without --home, or on a portable account (`claude setup-token`)",
                    ));
                }
                Some(home) => {
                    linked(home, &links)?;
                    home.to_string()
                }
                None => a.home.clone().unwrap_or_default(),
            };
            child.env(login.home_var, dir);
        }
    }
    Ok(())
}

/// `dir`, made if missing.
fn made(dir: &str) -> Result<(), Refused> {
    std::fs::create_dir_all(dir).map_err(|e| bad(format!("--home {dir}: {e}")))
}

/// Each login file linked into `home` where its engine's home holds it; a
/// link there already to the same file is left, anything else refused.
fn linked(home: &str, links: &[Link]) -> Result<(), Refused> {
    made(home)?;
    for link in links {
        let at = Path::new(home).join(link.as_);
        match std::fs::read_link(&at) {
            Ok(to) if to == Path::new(&link.path) => continue,
            Err(_) if std::fs::symlink_metadata(&at).is_err() => {}
            _ => {
                return Err(bad(format!(
                    "--home {home} already holds {}, not a link to {}: move it away, or give another --home",
                    link.as_, link.path
                )));
            }
        }
        if let Some(parent) = at.parent() {
            made(&parent.display().to_string())?;
        }
        std::os::unix::fs::symlink(&link.path, &at)
            .map_err(|e| bad(format!("linking {}: {e}", at.display())))?;
    }
    Ok(())
}

/// Whether a process of `pid` is alive (one of another user's is).
pub fn alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only asks whether the process exists.
    let found = unsafe { libc::kill(pid, 0) } == 0;
    found || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The child's pid, once started: where a signal is forwarded.
static CHILD: AtomicI32 = AtomicI32::new(0);

/// The signals passed on to the child.
const PASSED: [libc::c_int; 6] = [
    libc::SIGINT,
    libc::SIGQUIT,
    libc::SIGTERM,
    libc::SIGHUP,
    libc::SIGUSR1,
    libc::SIGUSR2,
];

/// Forwards each signal of [`PASSED`] a process sends stablehand to the
/// child. One the kernel sends (the terminal's ^C, its hangup: no sending
/// pid) reached the child too, its process group's, and is not sent again.
/// stablehand itself goes on waiting; the handlers are the default again in
/// the child, once it runs its program.
fn forward_signals() {
    extern "C" fn forward(sig: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
        // SAFETY: the kernel hands a valid siginfo to an SA_SIGINFO handler.
        let sender = unsafe { sender(info) };
        let child = CHILD.load(Ordering::SeqCst);
        if sender != 0 && child > 0 && sender != child {
            // SAFETY: kill is async-signal-safe.
            unsafe { libc::kill(child, sig) };
        }
    }
    for sig in PASSED {
        // SAFETY: a zeroed sigaction with an SA_SIGINFO handler that only
        // reads an atomic and calls kill.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = forward as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }
}

/// The pid that sent a signal; 0 for the kernel's.
#[cfg(target_os = "linux")]
unsafe fn sender(info: *const libc::siginfo_t) -> libc::pid_t {
    unsafe { (*info).si_pid() }
}

#[cfg(not(target_os = "linux"))]
unsafe fn sender(info: *const libc::siginfo_t) -> libc::pid_t {
    unsafe { (*info).si_pid }
}

/// The child's status as stablehand's own: its exit code, or the signal
/// that ended it raised on stablehand.
fn passed_on(status: ExitStatus) -> ExitCode {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        return ExitCode::from(code as u8);
    }
    let sig = status.signal().unwrap_or(libc::SIGTERM);
    // SAFETY: the default action back, then the same signal on ourselves.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
    ExitCode::from(128 + sig as u8)
}
