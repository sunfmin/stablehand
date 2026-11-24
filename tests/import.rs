//! `account import --from-mytokens`, through the binary, against the stand-ins
//! of mytokens and sesame.
mod support;

use serde_json::json;
use support::Register;

const TOKEN: &str = "sk-ant-oat01-imported-secret";

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
fn imports_each_entry_its_token_into_sesame_and_keeps_what_is_kept() {
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

    // A dry run says what it would do, reads no token and writes nothing.
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
                a["token"].clone(),
            )
        })
        .collect();
    assert_eq!(
        done,
        [
            (json!("claude-felix"), json!("would add"), json!(true)),
            (json!("codex"), json!("already kept"), json!(false)),
        ]
    );
    assert_eq!(r.ok(&["account", "list"]), before);
    assert!(
        !r.argv().contains("--field CLAUDE_CODE_OAUTH_TOKEN"),
        "{}",
        r.argv()
    );
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

    assert_eq!(r.sesame_token("stablehand-claude-felix").unwrap(), TOKEN);
    assert_eq!(
        r.ok(&["account", "show", "claude-felix"])["logged_in"],
        true
    );
    let argv = r.argv();
    assert!(!argv.contains(TOKEN), "{argv}");
    assert!(
        argv.contains("sesame login stablehand-claude-felix --token-stdin"),
        "{argv}"
    );
    let db = std::fs::read(r.dir.join("stablehand.db")).unwrap();
    assert!(!db.windows(TOKEN.len()).any(|w| w == TOKEN.as_bytes()));

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
    assert!(
        !after.lines().skip(calls).any(|l| l.starts_with("sesame")),
        "{after}"
    );
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
    assert_eq!(out["accounts"][1]["token"], false);
    assert!(!r.argv().contains("stablehand-codex"), "{}", r.argv());
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
    assert!(r.sesame_token("stablehand-claude-felix").is_none());

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
fn a_token_sesame_refuses_imports_that_account_on_the_next_run() {
    let r = Register::new("import-sesame-refuses");
    host_entries(&r);
    let args = ["account", "import", "--from-mytokens"];
    let e = r.refused_with(&args, &[("FAKE_SESAME_FAULT", "token-refused")], "", 1);
    assert!(
        e.contains("sesame kept no token for stablehand-claude-felix"),
        "{e}"
    );
    assert!(e.contains("account import --from-mytokens"), "{e}");

    let out = r.ok(&args);
    assert_eq!(out["accounts"][0]["result"], "added");
    assert_eq!(r.sesame_token("stablehand-claude-felix").unwrap(), TOKEN);
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

    // A token mytokens lists and does not give: that account is not added.
    let r = Register::new("import-token-unread");
    r.mytokens_entry("agent-account-claude-felix", &[("HARNESS", "claude-code")]);
    let unread = r.path("mytokens/agent-account-claude-felix/default/CLAUDE_CODE_OAUTH_TOKEN");
    std::fs::create_dir(unread).unwrap();
    let e = r.refused(&args, 1);
    assert!(
        e.contains("`mytokens get agent-account-claude-felix --account default --field CLAUDE_CODE_OAUTH_TOKEN` failed"),
        "{e}"
    );
    assert!(r.sesame_token("stablehand-claude-felix").is_none());
}
