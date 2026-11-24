//! An account's login, by its engine's own (docs/adr/0001 "Logins are the
//! engine's own"): the programs it runs, the engine's and sesame's. A token
//! never passes through stablehand: sesame reads it from the terminal or the
//! stdin stablehand was given, and no child is given it on its command line.
use crate::account::Account;
use std::io::IsTerminal;
use std::os::fd::AsFd;
use std::process::{Command, Stdio};

/// The sesame provider that holds a portable account's token.
pub fn provider(a: &Account) -> String {
    format!("stablehand-{}", a.name)
}

/// This machine's name, as `--machine` names it: `$STABLEHAND_MACHINE` (not
/// empty), else `hostname -s`.
fn this_machine() -> Result<String, String> {
    if let Some(m) = std::env::var("STABLEHAND_MACHINE")
        .ok()
        .filter(|m| !m.is_empty())
    {
        return Ok(m);
    }
    let out = Command::new("hostname")
        .arg("-s")
        .output()
        .map_err(|e| format!("hostname -s: {e}: set STABLEHAND_MACHINE to this machine's name"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// What a child prints goes to stablehand's stderr: its stdout is the answer's alone.
fn to_stderr() -> Result<Stdio, String> {
    let fd = std::io::stderr().as_fd().try_clone_to_owned();
    fd.map(Stdio::from).map_err(|e| format!("stderr: {e}"))
}

/// Runs `argv` as the person's own: their terminal, or no stdin without one.
fn run(argv: &[&str], env: Option<(&str, &str)>) -> Result<(), String> {
    let mut cmd = Command::new(argv[0]);
    cmd.args(&argv[1..]).stdout(to_stderr()?);
    if !std::io::stdin().is_terminal() {
        cmd.stdin(Stdio::null());
    }
    if let Some((k, v)) = env {
        cmd.env(k, v);
    }
    let line = argv.join(" ");
    let status = cmd.status().map_err(|e| missing(argv[0], &line, e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{line}` failed ({status}), as it said above: nothing was kept; run the login again"
        ))
    }
}

fn missing(program: &str, line: &str, e: std::io::Error) -> String {
    let fix = if program == "sesame" {
        "install it: `brew install sunfmin/tap/sesame`".to_string()
    } else {
        format!("install {program}, or put it on PATH")
    };
    format!("`{line}` could not start: {e}: {fix}")
}

/// One sesame call, on stablehand's stdin (the token's way in for `login`);
/// `quiet`, what it says is not shown.
fn sesame(args: &[&str], quiet: bool) -> Result<bool, String> {
    let mut cmd = Command::new("sesame");
    cmd.args(args).stdout(to_stderr()?);
    if quiet {
        cmd.stderr(Stdio::null());
    }
    let line = format!("sesame {}", args.join(" "));
    let status = cmd.status().map_err(|e| missing("sesame", &line, e))?;
    Ok(status.success())
}

/// Whether `a` is logged in: a bound login as the register recorded it, a
/// portable one as sesame says (`sesame token <provider> --json` answers).
pub fn logged_in(a: &Account) -> Result<bool, String> {
    if a.machine.is_some() {
        return Ok(a.logged_in_at.is_some());
    }
    let p = provider(a);
    let out = Command::new("sesame")
        .args(["token", &p, "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| missing("sesame", &format!("sesame token {p} --json"), e))?;
    Ok(out.status.success())
}

/// Logs `a` in by its engine's own login: a portable token into sesame
/// (`token`: pasted, not made by the engine's command), or a bound login in
/// its home on this machine. Answers the account as the register keeps it now.
pub fn login(a: &Account, token: bool, now: String) -> Result<Account, String> {
    let login = a.engine.login();
    let name = &a.name;
    let Some(machine) = &a.machine else {
        if !token {
            let Some(argv) = login.token else {
                return Err(format!(
                    "{} makes no long-lived token: paste one with `stablehand account login {name} --token`",
                    a.engine.name()
                ));
            };
            run(argv, None)?;
        }
        return keep_token(a, login.usage_host).map(|()| a.clone());
    };
    if token {
        return Err(format!(
            "account `{name}` is bound to {machine}: its engine logs in there, with no --token; drop --token, or `stablehand account edit {name} --unset machine --unset home` for a portable token"
        ));
    }
    let here = this_machine()?;
    if &here != machine {
        return Err(format!(
            "account `{name}` is bound to {machine} and this machine is {here} ($STABLEHAND_MACHINE, else `hostname -s`): run the login on {machine}"
        ));
    }
    let Some(home) = &a.home else {
        return Err(format!(
            "account `{name}` has no home to log in to: `stablehand account edit {name} --home <dir>`"
        ));
    };
    std::fs::DirBuilder::new()
        .recursive(true)
        .create(home)
        .map_err(|e| format!("{home}: {e}"))?;
    run(login.bound, Some((login.home_var, home)))?;
    Ok(Account {
        logged_in_at: Some(now),
        ..a.clone()
    })
}

/// The account's sesame provider, made where missing, then the token: sesame
/// asks for it on the terminal with hidden input, or reads stablehand's stdin.
fn keep_token(a: &Account, host: &str) -> Result<(), String> {
    let p = provider(a);
    if !sesame(&["provider", "show", &p], true)? {
        let url = format!("https://{host}");
        let about = format!(
            "stablehand account {}: a pasted {} token, no OAuth login",
            a.name,
            a.engine.name()
        );
        let args = [
            "provider",
            "add",
            &p,
            "--about",
            &about,
            "--auth-url",
            &url,
            "--token-url",
            &url,
            "--api-base",
            &url,
            "--host",
            host,
        ];
        if !sesame(&args, false)? {
            return Err(format!(
                "`sesame provider add {p}` failed: see `sesame provider show {p}`, then run the login again"
            ));
        }
    }
    let how = if std::io::stdin().is_terminal() {
        "--token"
    } else {
        "--token-stdin"
    };
    if sesame(&["login", &p, how], false)? {
        Ok(())
    } else {
        Err(format!(
            "sesame kept no token for {p}, as it said above: run `stablehand account login {} --token` again with the token",
            a.name
        ))
    }
}
