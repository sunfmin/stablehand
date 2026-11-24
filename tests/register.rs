//! The account register, run through the binary: each command's JSON and
//! exit code, against a config dir of the test's own.
mod support;

use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use support::Register;

fn account(name: &str, engine: &str, sessions: Value) -> Value {
    json!({
        "name": name, "engine": engine, "login": "portable", "machine": null,
        "sessions": sessions, "expires": null, "home": null, "off": null,
        "logged_in_at": null,
    })
}

#[test]
fn add_list_show_edit_and_rm() {
    let r = Register::new("crud");
    assert_eq!(r.ok(&["account", "list"]), json!({ "accounts": [] }));

    let claude = r.ok(&[
        "account",
        "add",
        "claude-felix",
        "--engine",
        "claude-code",
        "--expires",
        "2027-01-31",
    ]);
    let mut want = account("claude-felix", "claude-code", json!(2));
    want["expires"] = json!("2027-01-31");
    assert_eq!(claude, json!({ "account": want }));

    let codex = r.ok(&[
        "account",
        "add",
        "chatgpt",
        "--engine",
        "codex",
        "--machine",
        "mini",
        "--home",
        "/Users/agent/chatgpt",
    ]);
    let mut bound = account("chatgpt", "codex", Value::Null);
    bound["login"] = json!("bound");
    bound["machine"] = json!("mini");
    bound["home"] = json!("/Users/agent/chatgpt");
    assert_eq!(codex["account"], bound);
    let kimi = r.ok(&[
        "account",
        "add",
        "kimi-1",
        "--engine",
        "kimi",
        "--sessions",
        "4",
    ]);
    assert_eq!(kimi["account"], account("kimi-1", "kimi", json!(4)));

    assert_eq!(
        r.ok(&["account", "list"]),
        json!({ "accounts": [bound, want, kimi["account"]] })
    );
    assert_eq!(r.ok(&["account", "show", "chatgpt"])["account"], bound);

    // Only the terms given change; --unset clears; the account is judged as it would be.
    let edited = r.ok(&[
        "account",
        "edit",
        "claude-felix",
        "--sessions",
        "3",
        "--machine",
        "mini",
        "--home",
        "/Users/agent/claude",
        "--unset",
        "expires",
    ]);
    let mut moved = account("claude-felix", "claude-code", json!(3));
    moved["login"] = json!("bound");
    moved["machine"] = json!("mini");
    moved["home"] = json!("/Users/agent/claude");
    assert_eq!(edited["account"], moved);
    let portable = r.ok(&[
        "account", "edit", "chatgpt", "--engine", "pi", "--unset", "machine", "--unset", "home",
        "--unset", "sessions",
    ]);
    assert_eq!(portable["account"], account("chatgpt", "pi", Value::Null));
    assert_eq!(r.ok(&["account", "show", "claude-felix"])["account"], moved);

    // rm, run twice, leaves the same register.
    let gone = json!({ "name": "chatgpt", "removed": true });
    assert_eq!(r.ok(&["account", "rm", "chatgpt"]), gone);
    let again = json!({ "name": "chatgpt", "removed": false });
    assert_eq!(r.ok(&["account", "rm", "chatgpt"]), again);
    let fault = r.refused(&["account", "show", "chatgpt"], 1);
    assert!(fault.contains("stablehand account list"), "{fault}");
    assert_eq!(
        r.ok(&["account", "list"])["accounts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn refusals_name_the_fix_and_write_nothing() {
    let r = Register::new("refusals");
    r.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    // Each case: its arguments split at `|`, its exit code, and the parts of
    // its message; the second-to-last names every fault at once.
    let cases: &[(&str, i32, &[&str])] = &[
        ("account|add|x|--engine|gemini", 2, &["claude-code", "dsh"]),
        (
            "account|edit|claude-felix|--engine|gemini",
            2,
            &["possible values"],
        ),
        (
            "account|add|claude-felix|--engine|codex",
            1,
            &["stablehand account edit claude-felix"],
        ),
        (
            "account|add|c2|--engine|codex|--expires|2027-02-30",
            1,
            &["YYYY-MM-DD"],
        ),
        (
            "account|add|g|--engine|grok|--home|/h",
            1,
            &["needs --machine"],
        ),
        (
            "account|add|g|--engine|grok|--machine| ",
            1,
            &["--machine is empty"],
        ),
        (
            "account|edit|claude-felix|--machine|mini|--home|",
            1,
            &["--home is empty"],
        ),
        (
            "account|edit|nobody|--sessions|1",
            1,
            &["stablehand account add nobody"],
        ),
        ("off|nobody|--reason|x", 1, &["no account `nobody`"]),
        ("on|nobody", 1, &["no account `nobody`"]),
        ("off|claude-felix|--reason| ", 1, &["say why"]),
        ("account|add|y", 2, &["--engine"]),
        (
            "account|add|two words|--engine|grok|--machine|mini|--expires|2027-01-31|--sessions|0",
            1,
            &["not one word", "--sessions 0", "no --expires"],
        ),
    ];
    for (args, code, parts) in cases {
        let args: Vec<&str> = args.split('|').collect();
        let fault = r.refused(&args, *code);
        for part in *parts {
            assert!(fault.contains(part), "{args:?}: {part}: {fault}");
        }
    }
}

#[test]
fn off_keeps_who_when_and_why_and_on_forgets_it() {
    let r = Register::new("off");
    r.ok(&["account", "add", "grok-1", "--engine", "grok"]);
    let off = r.ok(&["off", "grok-1", "--reason", "card declined"]);
    let kept = &off["account"]["off"];
    assert_eq!(kept["by"], "tester");
    assert_eq!(kept["reason"], "card declined");
    let at = kept["at"].as_str().unwrap();
    assert!(
        at.len() == 20 && at.ends_with('Z') && at.as_bytes()[10] == b'T',
        "{at}"
    );
    assert_eq!(
        r.ok(&["account", "show", "grok-1"])["account"],
        off["account"]
    );

    let on = r.ok(&["on", "grok-1"]);
    assert_eq!(on["account"], account("grok-1", "grok", Value::Null));
    assert_eq!(r.ok(&["on", "grok-1"]), on);
    assert_eq!(
        r.ok(&["account", "show", "grok-1"])["account"],
        on["account"]
    );
}

#[test]
fn the_store_is_0600_in_a_0700_directory() {
    let r = Register::new("modes");
    r.ok(&["account", "list"]);
    let mode = |p: &PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let db = r.dir.join("stablehand.db");
    assert_eq!(mode(&r.dir), 0o700);
    assert_eq!(mode(&db), 0o600);

    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    r.ok(&["account", "list"]);
    assert_eq!(mode(&db), 0o600);
    // Only group's and other's bits are taken away.
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o700)).unwrap();
    r.ok(&["account", "list"]);
    assert_eq!(mode(&db), 0o700);
}

#[test]
fn a_store_that_is_no_database_is_refused_naming_its_file() {
    let r = Register::new("corrupt");
    std::fs::create_dir_all(&r.dir).unwrap();
    std::fs::write(r.dir.join("stablehand.db"), vec![7u8; 4096]).unwrap();
    let out = r.run(&["account", "list"]);
    assert_eq!(out.code, 1);
    let fault = out.json["error"].as_str().unwrap();
    assert!(
        fault.contains("stablehand.db") && fault.contains("not a database"),
        "{fault}"
    );
}

#[test]
fn help_and_version_are_text_not_refusals() {
    for args in [
        &["--help"][..],
        &["--version"],
        &["account", "add", "--help"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_stablehand"))
            .args(args)
            .output()
            .unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(out.status.success(), "{args:?}");
        assert!(
            stdout.starts_with("stablehand") || stdout.starts_with("Add an account"),
            "{args:?}: {stdout}"
        );
    }
}
