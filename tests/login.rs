//! `account login` and `account show`'s status, through the binary, against
//! the stand-ins of sesame and of the engines' login commands.
mod support;

use serde_json::json;
use support::Register;

const TOKEN: &str = "sk-ant-oat01-stand-in-secret";

#[test]
fn a_portable_token_reaches_sesame_on_stdin_and_no_argv() {
    let r = Register::new("portable");
    r.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    let show = r.ok(&["account", "show", "claude-felix"]);
    assert_eq!(show["logged_in"], false);

    // The engine's own token command runs first, then sesame reads the token.
    let stdin = format!("{TOKEN}\n");
    let args = [
        "account",
        "login",
        "claude-felix",
        "--expires",
        "2099-01-31",
    ];
    let out = r.run_with(&args, &[], &stdin);
    assert_eq!(out.code, 0, "{}", out.json);
    let answer = out.json;
    assert_eq!(answer["logged_in"], true);
    assert_eq!(answer["warning"], json!(null));
    assert!(answer["days_left"].as_i64().unwrap() > 26_000, "{answer}");
    assert_eq!(answer["account"]["expires"], "2099-01-31");
    assert_eq!(r.ok(&["account", "show", "claude-felix"]), answer);
    assert_eq!(r.sesame_token("stablehand-claude-felix").unwrap(), TOKEN);

    let argv = r.argv();
    let calls: Vec<&str> = argv.lines().collect();
    assert_eq!(calls[0], "sesame token stablehand-claude-felix --json");
    assert_eq!(calls[1], "claude setup-token");
    assert_eq!(calls[2], "sesame provider show stablehand-claude-felix");
    assert!(
        calls[3].starts_with("sesame provider add stablehand-claude-felix")
            && calls[3].ends_with("--host api.anthropic.com"),
        "{argv}"
    );
    assert_eq!(
        calls[4],
        "sesame login stablehand-claude-felix --token-stdin"
    );

    // Pasted again with --token: no engine, the provider kept, the new token in.
    let renewed = "sk-ant-oat01-renewed";
    let out = r.run_with(
        &["account", "login", "claude-felix", "--token"],
        &[],
        &format!("{renewed}\n"),
    );
    assert_eq!(out.code, 0, "{}", out.json);
    assert_eq!(r.sesame_token("stablehand-claude-felix").unwrap(), renewed);
    let argv = r.argv();
    assert_eq!(argv.matches("claude setup-token").count(), 1, "{argv}");
    assert_eq!(argv.matches("provider add").count(), 1, "{argv}");

    // The token is on no child's command line, and not in the register.
    for token in [TOKEN, renewed] {
        assert!(!argv.contains(token), "{argv}");
        let db = std::fs::read(r.dir.join("stablehand.db")).unwrap();
        assert!(!db.windows(token.len()).any(|w| w == token.as_bytes()));
    }

    // From 30 days ahead, and past it, show warns.
    r.ok(&["account", "edit", "claude-felix", "--expires", "2000-01-01"]);
    let show = r.ok(&["account", "show", "claude-felix"]);
    assert!(show["days_left"].as_i64().unwrap() < -9_000, "{show}");
    let warning = show["warning"].as_str().unwrap();
    assert!(warning.contains("expired on 2000-01-01"), "{warning}");
}

#[test]
fn a_refused_login_keeps_nothing() {
    let r = Register::new("refused");
    r.ok(&["account", "add", "kimi-1", "--engine", "kimi"]);
    r.ok(&["account", "add", "kimi-2", "--engine", "kimi"]);
    r.ok(&[
        "account",
        "add",
        "chatgpt",
        "--engine",
        "codex",
        "--machine",
        "mini",
    ]);
    r.ok(&[
        "account",
        "add",
        "boxed",
        "--engine",
        "codex",
        "--machine",
        "box",
        "--home",
        "/h",
    ]);
    let home = r.path("homes/codex-1");
    r.ok(&[
        "account",
        "add",
        "codex-1",
        "--engine",
        "codex",
        "--machine",
        "mini",
        "--home",
        &home,
    ]);
    r.ok(&[
        "account",
        "add",
        "kimi-b",
        "--engine",
        "kimi",
        "--machine",
        "mini",
        "--home",
        &r.path("homes/kimi-b"),
    ]);
    r.ok(&[
        "account",
        "add",
        "grok-b",
        "--engine",
        "grok",
        "--machine",
        "mini",
        "--home",
        "/dev/null/h",
    ]);
    let host = std::process::Command::new("hostname")
        .arg("-s")
        .output()
        .unwrap();
    let here = format!(
        "this machine is {}",
        String::from_utf8(host.stdout).unwrap().trim()
    );
    let none: &[(&str, &str)] = &[];
    // Each case: its arguments after `account login`, its environment, a part of its refusal.
    type Case<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)], &'a str);
    let cases: &[Case] = &[
        (&["kimi-1"], none, "stablehand account login kimi-1 --token"),
        (
            &["kimi-1", "--token", "--expires", "2027-01-31"],
            &[("FAKE_SESAME_FAULT", "token-refused")],
            "sesame kept no token",
        ),
        (&["chatgpt"], none, "stablehand account edit chatgpt --home"),
        (&["boxed"], none, "run the login on box"),
        (&["boxed"], &[("STABLEHAND_MACHINE", "")], &here),
        (&["kimi-b"], none, "install kimi, or put it on PATH"),
        (&["grok-b"], none, "/dev/null/h"),
        (
            &["kimi-2", "--token"],
            &[("FAKE_SESAME_FAULT", "add-refused")],
            "`sesame provider add stablehand-kimi-2` failed",
        ),
        (
            &["kimi-1", "--token"],
            &[("PATH", "/usr/bin:/bin")],
            "brew install sunfmin/tap/sesame",
        ),
        (&["codex-1", "--token"], none, "drop --token"),
        (
            &["codex-1", "--expires", "2027-01-31"],
            none,
            "no --expires",
        ),
        (
            &["codex-1"],
            &[("FAKE_ENGINE_FAULT", "fail")],
            "`codex login` failed",
        ),
        (&["nobody"], none, "no account `nobody`"),
    ];
    for (args, env, part) in cases {
        let args = [&["account", "login"], *args].concat();
        let fault = r.refused_with(&args, env, "a-token\n", 1);
        assert!(fault.contains(part), "{args:?}: {fault}");
    }
    assert_eq!(r.ok(&["account", "show", "kimi-1"])["logged_in"], false);
    assert_eq!(r.sesame_token("stablehand-kimi-1"), None);
}

#[test]
fn a_bound_login_runs_the_engine_in_its_home_and_is_recorded() {
    let r = Register::new("bound");
    let home = r.path("homes/chatgpt");
    r.ok(&[
        "account",
        "add",
        "chatgpt",
        "--engine",
        "codex",
        "--machine",
        "mini",
        "--home",
        &home,
    ]);
    assert_eq!(r.ok(&["account", "show", "chatgpt"])["logged_in"], false);

    let answer = r.ok(&["account", "login", "chatgpt"]);
    assert_eq!(answer["logged_in"], true);
    assert_eq!(answer["days_left"], json!(null));
    let at = answer["account"]["logged_in_at"].as_str().unwrap();
    assert!(at.len() == 20 && at.ends_with('Z'), "{at}");
    assert_eq!(r.argv(), format!("codex login CODEX_HOME={home}\n"));
    assert!(std::path::Path::new(&home).is_dir());
    assert_eq!(r.ok(&["account", "show", "chatgpt"]), answer);

    // Another home is another login: the record goes.
    let other = r.path("homes/other");
    let moved = r.ok(&["account", "edit", "chatgpt", "--home", &other]);
    assert_eq!(moved["account"]["logged_in_at"], json!(null));
    assert_eq!(r.ok(&["account", "show", "chatgpt"])["logged_in"], false);
    let sessions = r.ok(&["account", "edit", "chatgpt", "--sessions", "2"]);
    assert_eq!(sessions["account"]["logged_in_at"], json!(null));
    r.ok(&["account", "login", "chatgpt"]);
    let kept = r.ok(&["account", "edit", "chatgpt", "--sessions", "3"]);
    assert!(kept["account"]["logged_in_at"].is_string(), "{kept}");
}

#[test]
fn help_lists_each_engines_login() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_stablehand"))
        .args(["account", "login", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    for line in [
        "claude-code: bound `claude auth login`, CLAUDE_CONFIG_DIR; token `claude setup-token`",
        "codex: bound `codex login`, CODEX_HOME; token none: --token",
        "agy: bound `agy`, HOME;",
    ] {
        assert!(help.contains(line), "{line}: {help}");
    }
}
