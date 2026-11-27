//! Where stablehand finds an account's login, through the binary: a
//! portable token from the command the account names, a bound login in its
//! home's files. stablehand runs no login: `account login` is gone.
mod support;

use serde_json::json;
use std::time::{Duration, Instant};
use support::Register;

/// A token a shell would end a quote in or expand.
const TOKEN: &str = "sk-ant-oat01-it's$HOME`x`";

#[test]
fn a_portable_token_is_its_command_s_first_line_and_written_nowhere() {
    let r = Register::new("token-command");
    r.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    let show = r.ok(&["account", "show", "claude-felix"]);
    assert_eq!(show["logged_in"], false);
    assert_eq!(show["account"]["token_command"], json!(null));

    // The token is kept where the person keeps secrets (optokens' stand-in),
    // a second line after it, which is not the token.
    r.optokens_entry(
        "agent-account-claude-felix",
        &[("CLAUDE_CODE_OAUTH_TOKEN", &format!("{TOKEN}\nnot-it\n"))],
    );
    let cmd =
        "optokens get agent-account-claude-felix --account default --field CLAUDE_CODE_OAUTH_TOKEN";
    let edit = ["account", "edit", "claude-felix", "--token-command", cmd];
    assert_eq!(r.ok(&edit)["account"]["token_command"], cmd);
    let show = r.ok(&["account", "show", "claude-felix"]);
    assert_eq!(show["logged_in"], true, "{show}");
    assert!(!show.to_string().contains(TOKEN), "{show}");

    let answer = r.ok(&["credential", "claude-felix", "--json"]);
    assert_eq!(answer["env"], json!({"CLAUDE_CODE_OAUTH_TOKEN": TOKEN}));

    // It is on no child's command line, in no log, not in the register: the
    // one file that holds it is the person's own secret store.
    let argv = r.argv();
    assert!(
        argv.contains(&format!("optokens {}", &cmd["optokens ".len()..])),
        "{argv}"
    );
    assert!(!argv.contains(TOKEN) && !argv.contains("sk-ant"), "{argv}");
    assert!(!r.stderr().contains(TOKEN), "{}", r.stderr());
    let held = r.holding(TOKEN);
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(
        held[0].ends_with("optokens/agent-account-claude-felix/default/CLAUDE_CODE_OAUTH_TOKEN")
    );

    // A token far from its expiry counts its days and is not warned of.
    r.ok(&["account", "edit", "claude-felix", "--expires", "2099-01-31"]);
    let show = r.ok(&["account", "show", "claude-felix"]);
    assert!(show["days_left"].as_i64().unwrap() > 26_000, "{show}");
    assert!(show["days_left"].as_i64().unwrap() < 30_000, "{show}");
    assert_eq!(show["warning"], json!(null));

    // From 30 days ahead, and past it, show warns, naming where to renew.
    r.ok(&["account", "edit", "claude-felix", "--expires", "2000-01-01"]);
    let show = r.ok(&["account", "show", "claude-felix"]);
    assert!(show["days_left"].as_i64().unwrap() < -9_000, "{show}");
    let warning = show["warning"].as_str().unwrap();
    assert!(warning.contains("expired on 2000-01-01"), "{warning}");

    // Unset, the account is not logged in again.
    r.ok(&[
        "account",
        "edit",
        "claude-felix",
        "--unset",
        "token-command",
    ]);
    assert_eq!(
        r.ok(&["account", "show", "claude-felix"])["logged_in"],
        false
    );
}

#[test]
fn a_token_command_that_fails_prints_nothing_or_hangs_is_refused_naming_it() {
    let r = Register::new("token-command-refused");
    let bound = [("STABLEHAND_TOKEN_COMMAND_SECS", "1")];
    // Each case: the command, a part of its refusal.
    let cases = [
        // What it prints, out or err, is "leaked", a word not in the command.
        (
            "printf 'le%sked' a; printf 'le%sked' a >&2; exit 3",
            "exited 3",
        ),
        (
            "printf '\\n\\nsecond'",
            "printed no token on its first line",
        ),
        ("true", "printed no token on its first line"),
        ("sleep 30", "ran past 1s and was stopped"),
        (
            "sleep 30 & printf 'le%sked' a",
            "ran past 1s and was stopped",
        ),
        ("no-such-program-here", "exited 127"),
    ];
    for (i, (cmd, part)) in cases.iter().enumerate() {
        let name = format!("c{i}");
        r.ok(&["account", "add", &name, "--engine", "claude-code"]);
        r.ok(&["account", "edit", &name, "--token-command", cmd]);
        let started = Instant::now();
        let fault = r.refused_with(&["credential", &name], &bound, "", 2);
        assert!(started.elapsed() < Duration::from_secs(5), "{cmd}");
        assert!(fault.contains(part), "{cmd}: {fault}");
        assert!(fault.contains(&format!("`{cmd}`")), "{cmd}: {fault}");
        assert!(!fault.contains("leaked"), "{fault}");
        assert!(fault.contains(&format!("stablehand account edit {name} --token-command")));
        let show = r.run_with(&["account", "show", &name], &bound, "");
        assert_eq!(show.json["logged_in"], false, "{cmd}");
        // A reader given no token fails its try, naming the command too.
        let read = r.run_with(&["read", &name], &bound, "");
        assert_eq!(read.json["reads"][0]["outcome"], "failed", "{}", read.json);
        let error = read.json["reads"][0]["error"].as_str().unwrap();
        assert!(error.contains(part) && !error.contains("leaked"), "{error}");
    }
    assert!(!r.stderr().contains("leaked"), "{}", r.stderr());
}

#[test]
fn account_login_is_gone() {
    let r = Register::new("no-login");
    r.ok(&["account", "add", "a", "--engine", "claude-code"]);
    let fault = r.refused(&["account", "login", "a"], 2);
    assert!(fault.contains("unrecognized subcommand 'login'"), "{fault}");
    let help = r.raw(&["account", "--help"], &[], "").1;
    assert!(!help.contains("login "), "{help}");
}

#[test]
fn each_not_logged_in_refusal_names_its_engine_s_login_line() {
    let r = Register::new("login-lines");
    r.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    let homes = [
        ("codex", "codex"),
        ("claude-b", "claude-code"),
        ("kimi-b", "kimi"),
        ("dsh-b", "dsh"),
    ];
    for (name, engine) in homes {
        let home = r.path(&format!("homes/{name}"));
        let add = ["account", "add", name, "--engine", engine];
        r.ok(&[&add[..], &["--machine", "mini", "--home", &home]].concat());
    }
    let home = |n: &str| r.path(&format!("homes/{n}"));
    let cases = [
        ("claude-felix", "make a token with `claude setup-token`, keep it where you keep secrets, then `stablehand account edit claude-felix --token-command '<cmd>'`".to_string()),
        ("claude-felix", "security find-generic-password -s <service> -w".into()),
        ("claude-felix", "op read op://<vault>/<item>/<field>".into()),
        ("claude-felix", "optokens get <entry> --field <field>".into()),
        ("codex", format!("`CODEX_HOME={} codex login` on mini", home("codex"))),
        ("claude-b", format!("`CLAUDE_CONFIG_DIR={} claude auth login` on mini", home("claude-b"))),
        ("kimi-b", format!("`KIMI_CODE_HOME={} kimi login` on mini", home("kimi-b"))),
        ("dsh-b", format!("`DSH_HOME={} dsh tui` on mini", home("dsh-b"))),
    ];
    for (name, line) in &cases {
        let fault = r.refused(&["credential", name], 2);
        assert!(
            fault.contains("not logged in") && fault.contains(line.as_str()),
            "{name}: {fault}"
        );
        let engine = r.ok(&["account", "show", name])["account"]["engine"].clone();
        let pick = r.ok(&["pick", "--engine", engine.as_str().unwrap()]);
        let why = pick["why"].as_array().unwrap();
        let it = why.iter().find(|w| w["account"] == *name).unwrap();
        assert_eq!(it["reason"], "not_logged_in", "{pick}");
        assert!(
            it["why"].as_str().unwrap().contains(line.as_str()),
            "{pick}"
        );
    }
    // A bound home's reader that finds no login names the line too.
    let read = r.ok(&["read", "kimi-b", "--now"]);
    let error = read["reads"][0]["error"].as_str().unwrap();
    assert!(
        error.contains(&format!(
            "`KIMI_CODE_HOME={} kimi login` on mini",
            home("kimi-b")
        )),
        "{error}"
    );
    assert_eq!(r.argv(), "", "no engine ran");
}

#[test]
fn help_lists_each_engines_login() {
    let r = Register::new("login-help");
    let help = r.raw(&["account", "add", "--help"], &[], "").1;
    for line in [
        "claude-code: bound, logged in by `claude auth login` with CLAUDE_CONFIG_DIR its home; portable token made by `claude setup-token`, in CLAUDE_CODE_OAUTH_TOKEN",
        "codex: bound, logged in by `codex login` with CODEX_HOME its home; portable token none made, in none: bound only",
        "agy: bound, logged in by `agy` with HOME its home;",
        "op read op://<vault>/<item>/<field>",
        "stablehand runs no login",
    ] {
        assert!(help.contains(line), "{line}: {help}");
    }
}

#[test]
fn a_bound_home_holding_its_engines_login_file_is_logged_in() {
    let r = Register::new("adopted");
    let home = r.path("homes/codex");
    std::fs::create_dir_all(&home).unwrap();
    for (name, engine) in [("codex", "codex"), ("grok", "grok"), ("agy", "agy")] {
        let home = r.path(&format!("homes/{name}"));
        r.ok(&[
            "account",
            "add",
            name,
            "--engine",
            engine,
            "--machine",
            "mini",
            "--home",
            &home,
        ]);
    }

    // A home with no login file: not logged in, and nothing picked on it.
    let show = r.ok(&["account", "show", "codex"]);
    assert_eq!(show["logged_in"], false);
    assert_eq!(show["login_file"], json!(null));
    let pick = r.ok(&["pick", "--engine", "codex"]);
    assert_eq!(pick["why"][0]["reason"], "not_logged_in", "{pick}");
    assert!(r.raw(&["credential", "codex"], &[], "").0 != 0);

    // Its engine's login file in it (codex `auth.json`): logged in, as of
    // when the file was written, nothing written in the home.
    let file = format!("{home}/auth.json");
    std::fs::write(&file, "{}").unwrap();
    let show = r.ok(&["account", "show", "codex"]);
    assert_eq!(show["logged_in"], true, "{show}");
    assert_eq!(show["login_file"]["path"], file);
    let at = show["login_file"]["written_at"].as_str().unwrap();
    assert!(at.len() == 20 && at.ends_with('Z'), "{at}");
    assert_eq!(r.ok(&["pick", "--engine", "codex"])["account"], "codex");
    let credential = r.ok(&["credential", "codex", "--json"]);
    assert_eq!(credential["files"][0]["path"], file);
    let entries: Vec<_> = std::fs::read_dir(&home).unwrap().collect();
    assert_eq!(entries.len(), 1, "adopting wrote in the home");
    assert_eq!(r.argv(), "", "no engine ran");

    // grok's file is its home's `auth.json` too.
    std::fs::create_dir_all(r.path("homes/grok")).unwrap();
    std::fs::write(r.path("homes/grok/auth.json"), "{}").unwrap();
    assert_eq!(r.ok(&["pick", "--engine", "grok"])["account"], "grok");
    // agy's is under its HOME, where the person's own agy login is.
    let agy = r.path("homes/agy/.gemini/antigravity-cli");
    assert_eq!(r.ok(&["account", "show", "agy"])["logged_in"], false);
    std::fs::create_dir_all(&agy).unwrap();
    std::fs::write(format!("{agy}/antigravity-oauth-token"), "{}").unwrap();
    assert_eq!(r.ok(&["pick", "--engine", "agy"])["account"], "agy");

    // On another machine the file is not this one's to see.
    let elsewhere = [("STABLEHAND_MACHINE", "box")];
    let out = r.run_with(&["account", "show", "codex"], &elsewhere, "");
    assert_eq!(out.json["logged_in"], false, "{}", out.json);
}
