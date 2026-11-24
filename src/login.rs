//! An account's login, by its engine's own (docs/adr/0001 "Logins are the
//! engine's own"): the programs it runs, the engine's and sesame's. A token
//! never passes through stablehand: sesame reads it from the terminal or the
//! stdin stablehand was given, and no child is given it on its command line.
use crate::account::Account;
use crate::refusal::{Code, Refused};
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

/// Runs `argv`, reading `stdin`, with `env` set. `shown`: what it prints
/// goes to stablehand's stderr (its stdout is the answer's alone); else nowhere.
fn call(
    argv: &[&str],
    env: Option<(&str, &str)>,
    stdin: Stdio,
    shown: bool,
) -> Result<bool, String> {
    let mut cmd = Command::new(argv[0]);
    cmd.args(&argv[1..]).stdin(stdin).envs(env);
    if shown {
        let fd = std::io::stderr().as_fd().try_clone_to_owned();
        cmd.stdout(fd.map_err(|e| format!("stderr: {e}"))?);
    } else {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = cmd.status().map_err(|e| could_not_start(argv, e))?;
    Ok(status.success())
}

/// Why `argv` did not start, and how to make it.
fn could_not_start(argv: &[&str], e: std::io::Error) -> String {
    let fix = match argv[0] {
        "sesame" => "install it: `brew install sunfmin/tap/sesame`".to_string(),
        program => format!("install {program}, or put it on PATH"),
    };
    format!("`{}` could not start: {e}: {fix}", argv.join(" "))
}

/// Runs the engine's `argv` as the person's own: on their terminal, or with
/// no stdin without one, so it cannot read a token meant for sesame.
fn engine(argv: &[&str], env: Option<(&str, &str)>) -> Result<(), String> {
    let stdin = if std::io::stdin().is_terminal() {
        Stdio::inherit()
    } else {
        Stdio::null()
    };
    if call(argv, env, stdin, true)? {
        return Ok(());
    }
    Err(format!(
        "`{}` failed, as it said above: nothing was kept; run the login again",
        argv.join(" ")
    ))
}

fn sesame(args: &[&str], stdin: Stdio, shown: bool) -> Result<bool, String> {
    call(&[&["sesame"], args].concat(), None, stdin, shown)
}

/// Whether `a` is logged in: a bound login as the register recorded it, a
/// portable one as sesame says ([`token`]).
pub fn logged_in(a: &Account) -> Result<bool, String> {
    if a.machine.is_some() {
        return Ok(a.logged_in_at.is_some());
    }
    token(a).map(|t| t.is_some())
}

/// The token sesame keeps for `a`, if it keeps one: `sesame token <provider>
/// --json` answers it as `access_token`, on its stdout, read here and no
/// further.
pub fn token(a: &Account) -> Result<Option<String>, String> {
    let p = provider(a);
    let argv = ["sesame", "token", &p, "--json"];
    let out = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| could_not_start(&argv, e))?;
    if !out.status.success() {
        return Ok(None);
    }
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    match answer["access_token"].as_str() {
        Some(t) => Ok(Some(t.to_string())),
        None => Err(format!(
            "`sesame token {p} --json` answered no access_token: upgrade sesame (`brew upgrade sunfmin/tap/sesame`)"
        )),
    }
}

/// What a session on `a` starts with on this machine: its engine's variable
/// and its value; refused `no_session`, with its fix, an account no session
/// may start on here: off, of an engine with no token variable, not logged
/// in, with no home, or bound to another machine.
pub fn env(a: &Account) -> Result<(&'static str, String), Refused> {
    let no_session = |text: String| Err(Refused::new(Code::NoSession, text));
    let name = &a.name;
    if let Some(off) = &a.off {
        return no_session(format!(
            "account `{name}` is off ({} at {}: {}): start on another account, or `stablehand on {name}`",
            off.by, off.at, off.reason
        ));
    }
    let login = a.engine.login();
    let Some(machine) = &a.machine else {
        let Some(var) = login.token_var else {
            return no_session(format!(
                "{} takes no token from its environment: bind account `{name}` with `stablehand account edit {name} --machine <machine> --home <dir>`, then `stablehand account login {name}` there",
                a.engine.name()
            ));
        };
        return match token(a)? {
            Some(t) => Ok((var, t)),
            None => no_session(format!(
                "account `{name}` is not logged in: sesame holds no token for {}: `stablehand account login {name}`",
                provider(a)
            )),
        };
    };
    let Some(home) = &a.home else {
        return no_session(format!(
            "account `{name}` has no home: `stablehand account edit {name} --home <dir>`, then `stablehand account login {name}` on {machine}"
        ));
    };
    if a.logged_in_at.is_none() {
        return no_session(format!(
            "account `{name}` is not logged in: run `stablehand account login {name}` on {machine}"
        ));
    }
    let here = this_machine()?;
    if &here != machine {
        return no_session(format!(
            "account `{name}` is bound to {machine} and this machine is {here} ($STABLEHAND_MACHINE, else `hostname -s`): start its session on {machine}, or on another account"
        ));
    }
    Ok((login.home_var, home.clone()))
}

/// Logs `a` in by its engine's own login: a portable token into sesame
/// (`token`: pasted, not made by the engine's command), or a bound login in
/// its home on this machine. Answers the account as the register keeps it now.
pub fn login(a: &Account, token: bool, now: String) -> Result<Account, Refused> {
    let refused = |text: String| Err(Refused::new(Code::LoginRefused, text));
    let login = a.engine.login();
    let name = &a.name;
    let Some(machine) = &a.machine else {
        if !token {
            let Some(argv) = login.token else {
                return refused(format!(
                    "{} makes no long-lived token: paste one with `stablehand account login {name} --token`",
                    a.engine.name()
                ));
            };
            engine(argv, None)?;
        }
        keep_token(a, login.usage_host)?;
        return Ok(a.clone());
    };
    if token {
        return refused(format!(
            "account `{name}` is bound to {machine}: its engine logs in there, with no --token; drop --token, or `stablehand account edit {name} --unset machine --unset home` for a portable token"
        ));
    }
    let here = this_machine()?;
    if &here != machine {
        return refused(format!(
            "account `{name}` is bound to {machine} and this machine is {here} ($STABLEHAND_MACHINE, else `hostname -s`): run the login on {machine}"
        ));
    }
    let Some(home) = &a.home else {
        return refused(format!(
            "account `{name}` has no home to log in to: `stablehand account edit {name} --home <dir>`"
        ));
    };
    std::fs::create_dir_all(home).map_err(|e| format!("{home}: {e}"))?;
    engine(login.bound, Some((login.home_var, home)))?;
    Ok(Account {
        logged_in_at: Some(now),
        ..a.clone()
    })
}

/// The account's sesame provider, made where missing, then the token: sesame
/// asks for it on the terminal with hidden input, or reads stablehand's stdin.
fn keep_token(a: &Account, host: &str) -> Result<(), String> {
    let how = if std::io::stdin().is_terminal() {
        "--token"
    } else {
        "--token-stdin"
    };
    keep_token_read(a, host, how, Stdio::inherit())
}

/// The account's sesame provider, made where missing, then the token `from`
/// prints: its stdout is sesame's stdin, so the token is in no variable of
/// stablehand's.
pub fn keep_token_from(a: &Account, from: &[&str]) -> Result<(), String> {
    let mut child = Command::new(from[0])
        .args(&from[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("`{}` could not start: {e}", from.join(" ")))?;
    let out = child.stdout.take().expect("stdout is piped");
    let kept = keep_token_read(a, a.engine.login().usage_host, "--token-stdin", out.into());
    if !child.wait().map_err(|e| e.to_string())?.success() {
        return Err(format!(
            "`{}` failed, as it said above: sesame kept no token for {}",
            from.join(" "),
            provider(a)
        ));
    }
    kept
}

/// The provider, made where missing, then `sesame login <provider> <how>`
/// with the token on `token`.
fn keep_token_read(a: &Account, host: &str, how: &str, token: Stdio) -> Result<(), String> {
    let p = provider(a);
    if !sesame(&["provider", "show", &p], Stdio::null(), false)? {
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
        if !sesame(&args, Stdio::null(), true)? {
            return Err(format!(
                "`sesame provider add {p}` failed: see `sesame provider show {p}`, then run the login again"
            ));
        }
    }
    if sesame(&["login", &p, how], token, true)? {
        Ok(())
    } else {
        Err(format!(
            "sesame kept no token for {p}, as it said above: run `stablehand account login {} --token` again with the token",
            a.name
        ))
    }
}
