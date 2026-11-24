//! An account's login, by its engine's own (docs/adr/0001 "Logins are the
//! engine's own"): the programs it runs, the engine's and sesame's. A token
//! never passes through stablehand: sesame reads it from the terminal or the
//! stdin stablehand was given, and no child is given it on its command line.
use crate::account::{self, Account, Session};
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
fn token(a: &Account) -> Result<Option<String>, String> {
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

/// What a session on `a` starts with on this machine, each variable and its
/// value (account::session): `Err` when stablehand could not ask, `Ok(Err)`
/// a refusal, with its fix, of an account no session may start on here.
pub type Env = Result<Vec<(&'static str, String)>, String>;

pub fn env(a: &Account) -> Result<Env, String> {
    let name = &a.name;
    Ok(match account::session(a) {
        Err(refusal) => Err(refusal),
        Ok(Session::Token { var }) => match token(a)? {
            Some(t) => Ok(vec![(var, t)]),
            None => Err(format!(
                "account `{name}` is not logged in: sesame holds no token for {}: `stablehand account login {name}`",
                provider(a)
            )),
        },
        Ok(Session::Home { machine, var, home }) => {
            let here = this_machine()?;
            if here == machine {
                Ok(vec![(var, home.to_string())])
            } else {
                Err(format!(
                    "account `{name}` is bound to {machine} and this machine is {here} ($STABLEHAND_MACHINE, else `hostname -s`): start its session on {machine}, or on another account"
                ))
            }
        }
    })
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
            engine(argv, None)?;
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
    let how = if std::io::stdin().is_terminal() {
        "--token"
    } else {
        "--token-stdin"
    };
    if sesame(&["login", &p, how], Stdio::inherit(), true)? {
        Ok(())
    } else {
        Err(format!(
            "sesame kept no token for {p}, as it said above: run `stablehand account login {} --token` again with the token",
            a.name
        ))
    }
}
