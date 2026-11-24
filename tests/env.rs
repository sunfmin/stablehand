//! `env`: what a session on an account starts with, through the binary,
//! against the stand-in of sesame.
mod support;

use serde_json::json;
use support::Register;

/// A token a shell would end a quote in or expand: one value all the same.
const TOKEN: &str = "sk-ant-it's$HOME`x`";

/// Every file under `dir` whose bytes hold `secret`.
fn holding(dir: &std::path::Path, secret: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(holding(&path, secret));
        } else if let Ok(bytes) = std::fs::read(&path)
            && bytes.windows(secret.len()).any(|w| w == secret.as_bytes())
        {
            found.push(path.display().to_string());
        }
    }
    found
}

#[test]
fn a_portable_account_starts_with_its_token_from_sesame() {
    let r = Register::new("env-portable");
    r.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    let login = ["account", "login", "claude-felix", "--token"];
    assert_eq!(r.run_with(&login, &[], &format!("{TOKEN}\n")).code, 0);

    let (code, out) = r.raw(&["env", "claude-felix"], &[], "");
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        "export CLAUDE_CODE_OAUTH_TOKEN='sk-ant-it'\\''s$HOME`x`'\n"
    );
    // A shell that reads it has the token as it is, one value.
    let shell = std::process::Command::new("sh")
        .args(["-c", "eval \"$OUT\" && printenv CLAUDE_CODE_OAUTH_TOKEN"])
        .env("OUT", &out)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(shell.stdout).unwrap(),
        format!("{TOKEN}\n")
    );

    let answer = r.ok(&["env", "claude-felix", "--json"]);
    assert_eq!(
        answer,
        json!({"name": "claude-felix", "login": "portable", "env": {"CLAUDE_CODE_OAUTH_TOKEN": TOKEN}})
    );

    // Read from sesame's stdout: on no command line, in no file but sesame's.
    let argv = r.argv();
    assert!(!argv.contains(TOKEN), "{argv}");
    assert_eq!(
        argv.lines().last(),
        Some("sesame token stablehand-claude-felix --json")
    );
    let root = r.dir.parent().unwrap();
    let sesame = root.join("sesame/fake/tokens/stablehand-claude-felix");
    assert_eq!(holding(root, TOKEN), [sesame.display().to_string()]);
}

#[test]
fn a_bound_account_starts_in_its_home_on_its_machine() {
    let r = Register::new("env-bound");
    let home = r.path("homes/it's chatgpt");
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
    r.ok(&["account", "login", "chatgpt"]);
    let (code, out) = r.raw(&["env", "chatgpt"], &[], "");
    assert_eq!(code, 0, "{out}");
    let quoted = home.replace('\'', "'\\''");
    assert_eq!(out, format!("export CODEX_HOME='{quoted}'\n"));
    let answer = r.ok(&["env", "chatgpt", "--json"]);
    assert_eq!(
        answer,
        json!({"name": "chatgpt", "login": "bound", "env": {"CODEX_HOME": home}})
    );
}

#[test]
fn an_account_no_session_may_start_on_is_refused_with_its_fix() {
    let r = Register::new("env-refused");
    for name in ["claude-off", "claude-none"] {
        r.ok(&["account", "add", name, "--engine", "claude-code"]);
    }
    let login = ["account", "login", "claude-off", "--token"];
    assert_eq!(r.run_with(&login, &[], "sk-ant-x\n").code, 0);
    r.ok(&["off", "claude-off", "--reason", "card declined"]);
    r.ok(&["account", "add", "kimi-1", "--engine", "kimi"]);
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
        "codex-new",
        "--engine",
        "codex",
        "--machine",
        "mini",
    ]);
    let fresh = r.path("homes/codex-fresh");
    r.ok(&[
        "account",
        "add",
        "codex-fresh",
        "--engine",
        "codex",
        "--machine",
        "mini",
        "--home",
        &fresh,
    ]);
    r.ok(&["account", "login", "codex-1"]);
    let none: &[(&str, &str)] = &[];
    // Each case: the account, its environment, a part of its refusal.
    let cases: &[(&str, &[(&str, &str)], &str)] = &[
        ("claude-off", none, "card declined"),
        ("claude-off", none, "stablehand on claude-off"),
        ("claude-none", none, "stablehand account login claude-none"),
        ("kimi-1", none, "--machine <machine> --home <dir>"),
        (
            "codex-new",
            none,
            "stablehand account edit codex-new --home",
        ),
        ("codex-fresh", none, "stablehand account login codex-fresh"),
        (
            "codex-1",
            &[("STABLEHAND_MACHINE", "box")],
            "start its session on mini",
        ),
    ];
    for (name, env, part) in cases {
        for args in [&["env", name][..], &["env", name, "--json"]] {
            let fault = r.refused_with(args, env, "", 2);
            assert!(fault.contains(part), "{args:?}: {fault}");
        }
    }
    // Not an account of the register: a fault of the caller's, exit 1.
    let fault = r.refused(&["env", "nobody"], 1);
    assert!(fault.contains("stablehand account list"), "{fault}");
    // A failure to ask sesame is no refusal of the account: exit 1.
    let fault = r.refused_with(&["env", "claude-none"], &[("PATH", "/usr/bin:/bin")], "", 1);
    assert!(fault.contains("brew install sunfmin/tap/sesame"), "{fault}");
}
