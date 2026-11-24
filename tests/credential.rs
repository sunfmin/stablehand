//! `credential`: an account's login as a session's caller is handed it (a
//! token, or the login files to link), through the binary, against the
//! stand-in of sesame.
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
    let r = Register::new("credential-portable");
    r.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    let login = ["account", "login", "claude-felix", "--token"];
    assert_eq!(r.run_with(&login, &[], &format!("{TOKEN}\n")).code, 0);

    let (code, out) = r.raw(&["credential", "claude-felix"], &[], "");
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

    let answer = r.ok(&["credential", "claude-felix", "--json"]);
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
fn a_bound_account_answers_its_login_files_not_its_home() {
    let r = Register::new("credential-bound");
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
    let files = json!([{"path": format!("{home}/auth.json"), "as": "auth.json"}]);
    let want = json!({"name": "chatgpt", "login": "bound", "files": files});
    // No `export` lines: the session's home is its caller's, so there is no
    // variable to set; the answer is the same object with or without --json.
    for args in [
        &["credential", "chatgpt"][..],
        &["credential", "chatgpt", "--json"],
    ] {
        assert_eq!(r.ok(args), want, "{args:?}");
    }
}

/// Each engine's login file, under its home, as the engines' login facts
/// name it: what a caller links, and what makes a bound home logged in.
const LOGIN_FILES: &[(&str, &str)] = &[
    ("claude-code", ".credentials.json"),
    ("codex", "auth.json"),
    ("pi", "auth.json"),
    ("kimi", "credentials/kimi-code.json"),
    ("agy", ".gemini/antigravity-cli/antigravity-oauth-token"),
    ("grok", "auth.json"),
    ("dsh", ".credentials.yaml"),
];

#[test]
fn each_engine_s_login_file_is_both_what_is_linked_and_what_logs_a_home_in() {
    let r = Register::new("credential-files");
    let help = r.raw(&["credential", "--help"], &[], "").1;
    for (engine, file) in LOGIN_FILES {
        let home = r.path(&format!("homes/{engine}"));
        let add = ["account", "add", engine, "--engine", engine];
        r.ok(&[&add[..], &["--machine", "mini", "--home", &home]].concat());
        // Its home without the file: not logged in, and no credential.
        assert_eq!(r.ok(&["account", "show", engine])["logged_in"], false);
        let fault = r.refused_with(&["credential", engine], &[], "", 2);
        assert!(fault.contains("not logged in"), "{engine}: {fault}");

        let path = format!("{home}/{file}");
        std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
        std::fs::write(&path, "{}").unwrap();
        let show = r.ok(&["account", "show", engine]);
        assert_eq!(show["logged_in"], true, "{engine}: {show}");
        assert_eq!(show["login_file"]["path"], path, "{engine}");
        assert_eq!(
            r.ok(&["credential", engine])["files"],
            json!([{"path": path, "as": file}]),
            "{engine}"
        );
        assert!(
            help.contains(&format!("login file `{file}`")),
            "{engine}: {help}"
        );
    }
    assert_eq!(r.argv(), "", "no engine ran");
}

#[test]
fn an_account_no_session_may_start_on_is_refused_with_its_fix() {
    let r = Register::new("credential-refused");
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
    type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);
    let cases: &[Case] = &[
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
        for args in [&["credential", name][..], &["credential", name, "--json"]] {
            let fault = r.refused_with(args, env, "", 2);
            assert!(fault.contains(part), "{args:?}: {fault}");
        }
    }
    // Not an account of the register: a fault of the caller's, exit 1.
    let fault = r.refused(&["credential", "nobody"], 1);
    assert!(fault.contains("stablehand account list"), "{fault}");
    // A failure to ask sesame is no refusal of the account: exit 1.
    let fault = r.refused_with(
        &["credential", "claude-none"],
        &[("PATH", "/usr/bin:/bin")],
        "",
        1,
    );
    assert!(fault.contains("brew install sunfmin/tap/sesame"), "{fault}");
}
