//! `run`: an engine's own command started on the account stablehand picks,
//! through the binary, against the session stand-in of each engine
//! (tests/engines/fake-session) and mytokens' stand-in.
mod support;

use std::os::unix::process::ExitStatusExt;
use std::time::{Duration, Instant};
use support::Register;

/// A token a shell would end a quote in or expand.
const TOKEN: &str = "sk-ant-run's$HOME`x`";

/// `TOKEN`'s sha256, as the stand-in logs it.
fn token_sha() -> String {
    let out = std::process::Command::new("sh")
        .args(["-c", "printf '%s' \"$T\" | shasum -a 256 | cut -c1-64"])
        .env("T", TOKEN)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A register with a portable claude-code account `claude-felix` (its
/// token in mytokens' stand-in) and a codex account bound to this machine,
/// logged in, in `homes/codex`.
fn kept() -> impl Fn(&str) -> Register {
    |name| {
        let r = Register::new(name);
        let cmd = r.kept_token("agent-account-claude-felix", TOKEN);
        let add = ["account", "add", "claude-felix", "--engine", "claude-code"];
        r.ok(&[&add[..], &["--token-command", &cmd]].concat());
        let home = r.path("homes/codex");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(format!("{home}/auth.json"), "{}").unwrap();
        let add = [
            "account",
            "add",
            "codex",
            "--engine",
            "codex",
            "--machine",
            "mini",
        ];
        r.ok(&[&add[..], &["--home", &home]].concat());
        r
    }
}

/// `run args…` with the session stand-ins on PATH: exit code and stdout.
fn run(r: &Register, args: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let path = r.session_path();
    let env = [&[("PATH", path.as_str())][..], env].concat();
    r.raw(&[&["run"][..], args].concat(), &env, "")
}

/// The last line the stand-ins logged.
fn started(r: &Register) -> String {
    r.argv().lines().last().unwrap_or_default().to_string()
}

#[test]
fn a_command_starts_on_its_engine_s_pick_its_login_in_its_environment_alone() {
    let r = kept()("run-portable");
    let (code, out) = run(&r, &["claude", "-p", "hi"], &[("FAKE_EXIT", "7")]);
    // The child's own stdout and exit code.
    assert_eq!((code, out.as_str()), (7, "session on stdout\n"));
    let line = started(&r);
    assert!(line.starts_with("claude -p hi "), "{line}");
    assert!(
        line.contains(&format!("token_sha={}", token_sha())),
        "{line}"
    );
    assert!(!line.contains("CLAUDE_CONFIG_DIR"), "{line}");
    assert_eq!(r.stderr(), "stablehand: claude-code on claude-felix\n");
    // The account it runs on, by name, for its status line.
    assert!(line.contains(" STABLEHAND_ACCOUNT=claude-felix "), "{line}");

    // No token on any argv, stdout, stderr or log: only mytokens' own store holds it.
    assert!(!out.contains(TOKEN) && !r.argv().contains(TOKEN) && !r.stderr().contains(TOKEN));
    let held = r.holding(TOKEN);
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(
        held[0].contains("/mytokens/agent-account-claude-felix/"),
        "{held:?}"
    );
}

#[test]
fn no_command_starts_the_order_s_pick_its_model_flag_added_or_left() {
    let r = kept()("run-order");
    r.ok(&["order", "set", "codex@gpt-9", "claude-code@opus"]);
    assert_eq!(run(&r, &[], &[]).0, 0);
    let home = r.path("homes/codex");
    let line = started(&r);
    assert!(
        line.starts_with(&format!("codex --model gpt-9 CODEX_HOME={home} ")),
        "{line}"
    );

    r.ok(&["off", "codex", "--reason", "test"]);
    assert_eq!(run(&r, &[], &[]).0, 0);
    assert!(
        started(&r).starts_with("claude --model opus "),
        "{}",
        started(&r)
    );
    // A model the command names is left, and narrows the pick to it; the
    // same model named by --model too is one.
    assert_eq!(
        run(
            &r,
            &["--model", "haiku", "claude", "-m", "x", "--model", "haiku"],
            &[]
        )
        .0,
        0
    );
    assert!(
        started(&r).starts_with("claude -m x --model haiku "),
        "{}",
        started(&r)
    );
    assert_eq!(run(&r, &["claude", "-p", "x", "--model=sonnet"], &[]).0, 0);
    assert!(
        started(&r).starts_with("claude -p x --model=sonnet "),
        "{}",
        started(&r)
    );
    assert!(
        r.stderr()
            .ends_with("stablehand: claude-code on claude-felix (sonnet)\n"),
        "{}",
        r.stderr()
    );
}

#[test]
fn what_it_refuses_starts_nothing() {
    let r = kept()("run-refused");
    for (args, code, part) in [
        (&["--", "-p", "hi"][..], "bad_run", "no command"),
        (
            &["ls"],
            "bad_run",
            "no engine's program (`claude` claude-code, `codex` codex",
        ),
        (
            &["--engine", "codex", "claude"],
            "bad_run",
            "name one engine",
        ),
        (
            &["--account", "codex", "claude"],
            "bad_run",
            "name one engine",
        ),
        (&["--model", "a", "dsh"], "bad_run", "takes no model"),
        (
            &["--model", "a", "claude", "--model", "b"],
            "bad_run",
            "differ",
        ),
        (&["--account", "nope"], "no_account", "no account `nope`"),
    ] {
        let (exit, out) = run(&r, args, &[]);
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            (exit, json["code"].as_str()),
            (1, Some(code)),
            "{args:?}: {out}"
        );
        assert!(
            json["error"].as_str().unwrap().contains(part),
            "{args:?}: {out}"
        );
    }
    // None picked: pick's why and until, nothing started.
    r.ok(&["off", "claude-felix", "--reason", "card declined"]);
    let (exit, out) = run(&r, &["claude"], &[]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (exit, json["code"].as_str()),
        (1, Some("none_picked")),
        "{out}"
    );
    assert_eq!(json["why"][0]["reason"], "off", "{out}");
    assert!(json["until"].is_null(), "{out}");
    assert_eq!(r.argv(), "");
}

#[test]
fn home_links_a_bound_login_into_it_but_not_claude_code_s() {
    let r = kept()("run-home");
    let dir = r.path("sessions/one");
    let link = format!("{dir}/auth.json");
    let env = [("FAKE_LOGIN_FILE", link.as_str())];
    for _ in 0..2 {
        // Run again, the link already there is kept.
        assert_eq!(run(&r, &["--home", &dir, "codex"], &env).0, 0);
        let line = started(&r);
        let want = format!("CODEX_HOME={dir} HOME=");
        assert!(line.contains(&want), "{line}");
        assert!(
            line.ends_with(&format!("login->{}/auth.json", r.path("homes/codex"))),
            "{line}"
        );
    }
    let other = r.path("sessions/two");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(format!("{other}/auth.json"), "{}").unwrap();
    let (exit, out) = run(&r, &["--home", &other, "codex"], &[]);
    assert!(
        exit == 1 && out.contains("already holds auth.json"),
        "{out}"
    );
    // A link to another login is no link to the account's.
    let third = r.path("sessions/three");
    std::fs::create_dir_all(&third).unwrap();
    std::os::unix::fs::symlink(format!("{other}/auth.json"), format!("{third}/auth.json")).unwrap();
    let (exit, out) = run(&r, &["--home", &third, "codex"], &[]);
    assert!(
        exit == 1 && out.contains("already holds auth.json"),
        "{out}"
    );

    let home = r.path("homes/claude");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(format!("{home}/.credentials.json"), "{}").unwrap();
    let add = [
        "account",
        "add",
        "claude-2",
        "--engine",
        "claude-code",
        "--machine",
        "mini",
    ];
    r.ok(&[&add[..], &["--home", &home]].concat());
    let (exit, out) = run(
        &r,
        &["--account", "claude-2", "--home", &dir, "claude"],
        &[],
    );
    assert!(exit == 1 && out.contains("docs/adr/0002"), "{out}");
    // Without --home it starts on the account's own, given no token, even
    // one stablehand was given.
    let given = [("CLAUDE_CODE_OAUTH_TOKEN", "x")];
    assert_eq!(run(&r, &["--account", "claude-2", "claude"], &given).0, 0);
    let line = started(&r);
    assert!(
        line.contains(&format!("CLAUDE_CONFIG_DIR={home} ")),
        "{line}"
    );
    assert!(!line.contains("token_sha"), "{line}");
}

/// The pid the stand-in wrote to `file`, once it has.
fn pid_in(file: &str) -> i32 {
    let at = Instant::now();
    loop {
        if let Some(pid) = std::fs::read_to_string(file)
            .ok()
            .and_then(|s| s.trim().parse().ok())
        {
            return pid;
        }
        assert!(at.elapsed() < Duration::from_secs(10), "no session started");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether `pick` takes claude-felix, its pace with the sessions running.
fn takes(r: &Register) -> bool {
    let p = r.ok(&["pick", "--engine", "claude-code"]);
    p["account"] == "claude-felix"
}

#[test]
fn a_session_is_counted_while_it_runs_and_signals_pass_through() {
    let r = kept()("run-sessions");
    // Its five-hour window 60% used two thirds in: at its rate one session
    // ends it 90% used, two run it out (the pace counts those running).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let limits = format!(
        r#"{{"rate_limits":{{"five_hour":{{"used_percentage":60,"resets_at":{}}}}}}}"#,
        now + 6000
    );
    let (code, _) = r.raw(&["statusline", "--account", "claude-felix"], &[], &limits);
    assert_eq!(code, 0);
    assert!(takes(&r), "none running: it takes one");
    let hold = r.path("hold");
    let pids = r.path("pid");
    let env = [
        ("FAKE_HOLD", hold.as_str()),
        ("FAKE_PID_FILE", pids.as_str()),
    ];

    // A TERM to stablehand reaches the child; the child's end is stablehand's.
    std::fs::write(&hold, "").unwrap();
    let mut it = r.spawn(&["run", "claude"], &env);
    let child = pid_in(&pids);
    assert!(
        !takes(&r),
        "a session runs: one more would run its window out"
    );
    unsafe { libc::kill(it.id() as i32, libc::SIGTERM) };
    let status = it.wait().unwrap();
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
    assert_ne!(unsafe { libc::kill(child, 0) }, 0, "the child is gone");
    assert_eq!(
        sessions_kept(&r),
        0,
        "its session was dropped when it ended"
    );
    assert!(takes(&r));

    // A run killed outright leaves its session kept while the child lives,
    // dropped once its pid is found dead.
    std::fs::remove_file(&pids).unwrap();
    let mut it = r.spawn(&["run", "claude"], &env);
    let child = pid_in(&pids);
    it.kill().unwrap();
    it.wait().unwrap();
    assert!(!takes(&r), "its child still runs");
    unsafe { libc::kill(child, libc::SIGKILL) };
    let at = Instant::now();
    while unsafe { libc::kill(child, 0) } == 0 && at.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(takes(&r), "a dead session is not counted");
    assert_eq!(sessions_kept(&r), 0, "and dropped");
    std::fs::remove_file(&hold).unwrap();
}

/// The sessions the register keeps.
fn sessions_kept(r: &Register) -> i64 {
    let db = rusqlite::Connection::open(r.dir.join("stablehand.db")).unwrap();
    db.query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn a_signal_the_child_sends_stablehand_is_not_sent_back() {
    let r = kept()("run-child-signal");
    // The stand-in sends USR1 to its parent, stablehand, then exits 4: a
    // USR1 sent back to it would end it by that signal instead.
    let (code, _) = run(
        &r,
        &["claude"],
        &[("FAKE_SIGNAL_PARENT", "USR1"), ("FAKE_EXIT", "4")],
    );
    assert_eq!(code, 4);
}
