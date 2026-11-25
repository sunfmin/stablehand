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
#[derive(clap::Args)]
pub struct Asked {
    /// The engine, where the command is not its program.
    #[arg(long, value_enum, verbatim_doc_comment)]
    pub engine: Option<Engine>,
    /// Start on this account only.
    #[arg(long, verbatim_doc_comment)]
    pub account: Option<String>,
    /// Start on this model only, by the id its engine starts it with.
    #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new(), verbatim_doc_comment)]
    pub model: Option<String>,
    /// The engine's home: its home variable set to it, a bound account's login files linked into it; made if missing.
    #[arg(long, verbatim_doc_comment)]
    pub home: Option<String>,
    /// Start without the engine's kept command (`stablehand command show`): its bare program, or the command as given.
    #[arg(long, verbatim_doc_comment)]
    pub bare: bool,
    /// The engine's command and its arguments, after `--` where they start with `-`.
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub command: Vec<String>,
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
    let model = match (asked.model, in_command.clone()) {
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
    let mut accounts = match account {
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
    let model = picked["model"].as_str();
    let at = accounts
        .iter()
        .position(|a| picked["account"] == a.name.as_str());
    let a = accounts.swap_remove(at.ok_or("pick answered no account it was given".to_string())?);
    let engine = a.engine;
    // A model the command names is left as it is.
    let added = model.filter(|_| in_command.is_none());
    let kept = match asked.bare {
        true => Vec::new(),
        false => store.command(engine)?,
    };
    let (line, used) = command::line(engine, &asked.command, &kept, added);
    let mut child = Command::new(&line[0]);
    child.args(&line[1..]);
    hand_login(&mut child, &a, asked.home.as_deref())?;
    child.env(crate::statusline::ACCOUNT_VAR, &a.name);
    let said = model.map_or(String::new(), |m| format!(" ({m})"));
    let kept = if used { ", kept command" } else { "" };
    eprintln!("stablehand: {} on {}{said}{kept}", engine.name(), a.name);
    // Held from before the child starts until its pid is known, so each is
    // either stablehand's own (nothing started) or the child's.
    let unheld = hold_signals();
    // SAFETY: between fork and exec it only calls release_signals, which
    // calls pthread_sigmask, async-signal-safe.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut child, move || {
            release_signals(unheld);
            Ok(())
        });
    }
    let spawned = child.spawn();
    if let Ok(started) = &spawned {
        CHILD.store(started.id() as i32, Ordering::SeqCst);
        forward_signals();
    }
    release_signals(unheld);
    let mut started =
        spawned.map_err(|e| format!("`{}` did not start: {e}: is it on PATH?", line[0]))?;
    let pid = started.id();
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

/// The child's pid: where a signal is forwarded, set before any is.
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
        if sender != 0 && sender != child {
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

/// Blocks the signals of [`PASSED`]; answers the mask to restore (the
/// child restores it before it runs its program).
fn hold_signals() -> libc::sigset_t {
    // SAFETY: sigset_t is plain data, filled by sigemptyset and sigaddset.
    unsafe {
        let mut held: libc::sigset_t = std::mem::zeroed();
        let mut before: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut held);
        for sig in PASSED {
            libc::sigaddset(&mut held, sig);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &held, &mut before);
        before
    }
}

/// Restores the mask [`hold_signals`] answered: a signal held meanwhile is
/// delivered now.
fn release_signals(before: libc::sigset_t) {
    // SAFETY: `before` is a mask pthread_sigmask filled.
    unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &before, std::ptr::null_mut()) };
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
    // raise returns only for a signal whose default ends nothing, and no
    // such signal ended the child.
    ExitCode::FAILURE
}
