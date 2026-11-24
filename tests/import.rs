//! `account import --from-mytokens`, through the binary, against the stand-ins
//! of mytokens.
mod support;

use serde_json::json;
use support::Register;

const TOKEN: &str = "sk-ant-oat01-imported-secret";
/// The token command a portable entry's account is given.
const FELIX: &str =
    "mytokens get agent-account-claude-felix --account default --field CLAUDE_CODE_OAUTH_TOKEN";

fn host_entries(r: &Register) {
    r.mytokens_entry(
        "agent-account-claude-felix",
        &[
            ("HARNESS", "claude-code"),
            ("CLAUDE_CODE_OAUTH_TOKEN", TOKEN),
        ],
    );
    r.mytokens_entry(
        "agent-account-codex",
        &[("HARNESS", "codex"), ("MACHINE", "mini")],
    );
    r.mytokens_entry("huggingface", &[("HF_TOKEN", "hf-not-an-account")]);
}

#[test]
fn imports_each_entry_its_token_command_and_keeps_what_is_kept() {
    let r = Register::new("import");
    host_entries(&r);
    r.ok(&[
        "account",
        "add",
        "codex",
        "--engine",
        "codex",
        "--sessions",
        "1",
    ]);

    // A dry run says what it would do and writes nothing.
    let before = r.ok(&["account", "list"]);
    let dry = r.ok(&["account", "import", "--from-mytokens", "--dry-run"]);
    assert_eq!(dry["dry_run"], true);
    let done: Vec<_> = dry["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["account"]["name"].clone(),
                a["result"].clone(),
                a["account"]["token_command"].clone(),
            )
        })
        .collect();
    assert_eq!(
        done,
        [
            (json!("claude-felix"), json!("would add"), json!(FELIX)),
            (json!("codex"), json!("already kept"), json!(null)),
        ]
    );
    assert_eq!(r.ok(&["account", "list"]), before);
    assert!(!r.argv().contains(FELIX), "{}", r.argv());
    assert!(!dry.to_string().contains(TOKEN));

    let out = r.ok(&["account", "import", "--from-mytokens"]);
    assert_eq!(out["dry_run"], false);
    let felix = &out["accounts"][0];
    assert_eq!(felix["from"], "agent-account-claude-felix/default");
    assert_eq!(felix["result"], "added");
    assert_eq!(felix["account"]["engine"], "claude-code");
    assert_eq!(felix["account"]["login"], "portable");
    assert_eq!(felix["account"]["sessions"], 2);
    // codex is left as the register keeps it, not as mytokens has it.
    assert_eq!(out["accounts"][1]["account"]["login"], "portable");
    assert_eq!(out["accounts"][1]["account"]["sessions"], 1);
    assert!(!out.to_string().contains(TOKEN));
    // Its token is not read: it stays in mytokens, read when it is needed.
    assert_eq!(felix["account"]["token_command"], FELIX);
    assert!(!r.argv().contains(FELIX), "{}", r.argv());
    let credential = r.ok(&["credential", "claude-felix", "--json"]);
    assert_eq!(credential["env"]["CLAUDE_CODE_OAUTH_TOKEN"], TOKEN);
    let argv = r.argv();
    assert!(!argv.contains(TOKEN), "{argv}");
    let held = r.holding(TOKEN);
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(held[0].contains("/mytokens/"), "{held:?}");

    // Again: everything is kept, nothing more is read or written.
    let calls = argv.lines().count();
    let again = r.ok(&["account", "import", "--from-mytokens"]);
    let results: Vec<_> = again["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["result"].clone())
        .collect();
    assert_eq!(results, [json!("already kept"), json!("already kept")]);
    let after = r.argv();
    // mytokens is only read: listed and got, never put or removed.
    for call in after.lines().filter(|l| l.starts_with("mytokens ")) {
        assert!(
            call == "mytokens list" || call.starts_with("mytokens get "),
            "{call}"
        );
    }
    assert!(!after.lines().skip(calls).any(|l| l == FELIX), "{after}");

    // An account added by hand with no token command is given its entry's.
    let r = Register::new("import-command");
    host_entries(&r);
    r.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    let args = ["account", "import", "--from-mytokens"];
    let dry = r.ok(&[&args[..], &["--dry-run"]].concat());
    assert_eq!(dry["accounts"][0]["result"], "would give its token command");
    let out = r.ok(&args);
    assert_eq!(out["accounts"][0]["result"], "given its token command");
    let show = r.ok(&["account", "show", "claude-felix"]);
    assert_eq!(show["account"]["token_command"], FELIX);
    assert_eq!(show["logged_in"], true);
    // One named by hand is kept.
    r.ok(&[
        "account",
        "edit",
        "claude-felix",
        "--token-command",
        "op read op://v/i/f",
    ]);
    assert_eq!(r.ok(&args)["accounts"][0]["result"], "already kept");
}

#[test]
fn a_bound_entry_is_imported_bound_to_its_machine() {
    let r = Register::new("import-bound");
    host_entries(&r);
    let out = r.ok(&["account", "import", "--from-mytokens"]);
    let codex = &out["accounts"][1]["account"];
    assert_eq!(codex["name"], "codex");
    assert_eq!(codex["login"], "bound");
    assert_eq!(codex["machine"], "mini");
    assert_eq!(codex["token_command"], json!(null));
}

#[test]
fn an_entry_it_cannot_import_refuses_all_and_writes_nothing() {
    let r = Register::new("import-refused");
    host_entries(&r);
    r.mytokens_entry("agent-account-mystery", &[("HARNESS", "cursor")]);
    r.mytokens_entry(
        "agent-account-both",
        &[
            ("HARNESS", "claude-code"),
            ("MACHINE", "mini"),
            ("CLAUDE_CODE_OAUTH_TOKEN", TOKEN),
        ],
    );
    let e = r.refused(&["account", "import", "--from-mytokens"], 1);
    assert!(
        e.contains("agent-account-mystery/default: HARNESS `cursor` is no engine"),
        "{e}"
    );
    assert!(
        e.contains("agent-account-both/default: a token and a MACHINE"),
        "{e}"
    );
    assert_eq!(r.ok(&["account", "list"])["accounts"], json!([]));

    // mytokens failing to answer a field: nothing is imported.
    let r = Register::new("import-get-fails");
    host_entries(&r);
    let args = ["account", "import", "--from-mytokens"];
    let e = r.refused_with(&args, &[("FAKE_MYTOKENS_FAULT", "get-fails")], "", 1);
    assert!(
        e.contains("mytokens get agent-account-claude-felix --account default --field HARNESS"),
        "{e}"
    );
}

#[test]
fn what_mytokens_cannot_answer_is_named_with_its_fix() {
    let r = Register::new("import-no-mytokens");
    let args = ["account", "import", "--from-mytokens"];
    let e = r.refused_with(&args, &[("PATH", "/usr/bin:/bin")], "", 1);
    assert!(e.contains("`mytokens` could not start"), "{e}");
    assert!(e.contains("install mytokens"), "{e}");

    // A name the register refuses is named with the entry it came from.
    r.mytokens_entry("agent-account-two words", &[("HARNESS", "codex")]);
    let e = r.refused(&args, 1);
    assert!(
        e.contains("agent-account-two words/default: account name `two words` is not one word"),
        "{e}"
    );

    // A token mytokens lists and does not give: the account is added, the
    // import reads no token, and its credential is refused naming the command.
    let r = Register::new("import-token-unread");
    r.mytokens_entry("agent-account-claude-felix", &[("HARNESS", "claude-code")]);
    let unread = r.path("mytokens/agent-account-claude-felix/default/CLAUDE_CODE_OAUTH_TOKEN");
    std::fs::create_dir(unread).unwrap();
    assert_eq!(r.ok(&args)["accounts"][0]["result"], "added");
    let e = r.refused(&["credential", "claude-felix"], 2);
    assert!(e.contains(&format!("`{FELIX}` exited 1")), "{e}");
}

#[test]
fn a_bound_account_is_given_the_workers_home_and_a_second_run_fills_it_in() {
    let r = Register::new("import-home");
    host_entries(&r);
    r.mytokens_entry(
        "agent-account-grok",
        &[("HARNESS", "grok"), ("MACHINE", "mini")],
    );
    let home = r.path("home");
    let env = [("HOME", home.as_str())];
    let args = ["account", "import", "--from-mytokens"];
    // An earlier import left codex bound with no home; grok was given one by hand.
    r.ok(&[
        "account",
        "add",
        "codex",
        "--engine",
        "codex",
        "--machine",
        "mini",
    ]);
    r.ok(&[
        "account",
        "add",
        "grok",
        "--engine",
        "grok",
        "--machine",
        "mini",
        "--home",
        "/h/grok",
    ]);

    let dry = r
        .run_with(&[&args[..], &["--dry-run"]].concat(), &env, "")
        .json;
    assert_eq!(dry["accounts"][1]["result"], "would give its home", "{dry}");
    assert_eq!(
        r.ok(&["account", "show", "codex"])["account"]["home"],
        json!(null)
    );

    let out = r.run_with(&args, &env, "").json;
    let codex = &out["accounts"][1];
    assert_eq!(codex["result"], "given its home", "{out}");
    let workers = format!("{home}/.agent-accounts/codex");
    assert_eq!(codex["account"]["home"], workers);
    assert_eq!(out["accounts"][2]["result"], "already kept");
    assert_eq!(out["accounts"][2]["account"]["home"], "/h/grok");
    assert_eq!(
        r.ok(&["account", "show", "codex"])["account"]["home"],
        workers
    );
    // Nothing is made in the home: it is the workers', and its login's.
    assert!(!std::path::Path::new(&workers).exists());

    // Run again: everything is kept as it is now.
    let again = r.run_with(&args, &env, "").json;
    assert_eq!(again["accounts"][1]["result"], "already kept", "{again}");
    assert_eq!(again["accounts"][1]["account"]["home"], workers);

    // Its login file there: logged in, picked.
    std::fs::create_dir_all(&workers).unwrap();
    std::fs::write(format!("{workers}/auth.json"), "{}").unwrap();
    assert_eq!(r.ok(&["account", "show", "codex"])["logged_in"], true);
    assert_eq!(
        r.ok(&["pick", "--engine", "codex"]),
        json!({"account": "codex"})
    );

    // A new bound account is added with it; a portable one has none.
    let r = Register::new("import-home-new");
    host_entries(&r);
    let out = r.run_with(&args, &env, "").json;
    assert_eq!(out["accounts"][1]["result"], "added");
    assert_eq!(out["accounts"][1]["account"]["home"], workers);
    assert_eq!(out["accounts"][0]["account"]["home"], json!(null));
}
