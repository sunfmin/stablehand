//! `account scan`, through the binary, over a fixture home tree and the
//! stand-in of mytokens.
mod support;

use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};
use support::Register;

/// Every identity and token the fixture's login files and mytokens hold:
/// none may reach stdout, stderr or the register.
const SECRETS: &[&str] = &[
    "UUID-FELIX-SECRET",
    "ORG-SECRET",
    "sk-ant-ort01-refresh-SECRET",
    "CHATGPT-SECRET",
    "rt-codex-SECRET",
    "rt-pi-SECRET",
    "GROK-USER-SECRET",
    "opaque-kimi-SECRET",
    "OTHER-CHATGPT-SECRET",
    TOKEN,
];
const TOKEN: &str = "sk-ant-oat01-portable-SECRET";

/// `rel` under `dir` holding `text`, written at `at` seconds since 1970.
fn file(dir: &str, rel: &str, text: &str, at: Option<u64>) {
    let path = Path::new(dir).join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    if let Some(at) = at {
        let f = std::fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(UNIX_EPOCH + Duration::from_secs(at))
            .unwrap();
    }
}

/// A home tree: several engines; one Claude subscription in ~/.claude and
/// in a home given by --path; pi and codex on one ChatGPT account; kimi in a
/// format stablehand does not know; factorium's and Guild's homes, which
/// scan does not look in.
fn tree(r: &Register) -> (String, String) {
    let home = r.path("home");
    let claude = r.path("work-claude");
    let account = r#"{"oauthAccount": {"accountUuid": "UUID-FELIX-SECRET", "organizationUuid": "ORG-SECRET"}}"#;
    let login = r#"{"claudeAiOauth": {"refreshToken": "sk-ant-ort01-refresh-SECRET"}}"#;
    file(&home, ".claude/.credentials.json", login, Some(1_000));
    file(&home, ".claude.json", account, None);
    file(&claude, ".credentials.json", login, Some(2_000));
    file(&claude, ".claude.json", account, None);
    let codex =
        r#"{"tokens": {"account_id": "CHATGPT-SECRET", "refresh_token": "rt-codex-SECRET"}}"#;
    file(&home, ".codex/auth.json", codex, Some(3_000));
    let pi = r#"{"openai-codex": {"accountId": "CHATGPT-SECRET", "refresh": "rt-pi-SECRET"}}"#;
    file(&home, ".pi/agent/auth.json", pi, Some(2_500));
    let grok = r#"{"https://auth.x.ai::e1": {"user_id": "GROK-USER-SECRET"}}"#;
    file(&home, ".grok/auth.json", grok, Some(4_000));
    let kimi = r#"{"access_token": "opaque-kimi-SECRET"}"#;
    file(
        &home,
        ".kimi-code/credentials/kimi-code.json",
        kimi,
        Some(4_000),
    );
    let other = r#"{"tokens": {"account_id": "OTHER-CHATGPT-SECRET"}}"#;
    file(&home, ".agent-accounts/codex/auth.json", other, None);
    file(&home, ".guild/engines/codex/auth.json", other, None);
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
    (home, claude)
}

/// Each login as `(engine, home, status, account)`, and each token as
/// `(entry, status, account)`.
fn said(out: &Value) -> (Vec<Value>, Vec<Value>) {
    let logins = out["logins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| json!([l["engine"], l["home"], l["status"], l["account"]]));
    let tokens = out["tokens"].as_array().unwrap().iter();
    let tokens = tokens.map(|t| json!([t["entry"], t["status"], t["account"]]));
    (logins.collect(), tokens.collect())
}

#[test]
fn scan_finds_one_account_per_subscription_and_add_registers_the_new_ones() {
    let r = Register::new("scan");
    let (home, claude) = tree(&r);
    // A grok account kept elsewhere: the one found here is named grok-2.
    let elsewhere = ["--machine", "studio", "--home", "/elsewhere"];
    r.ok(&[
        &["account", "add", "grok", "--engine", "grok"][..],
        &elsewhere,
    ]
    .concat());
    let env = [("HOME", home.as_str())];
    let scan = |extra: &[&str]| {
        let args = [&["account", "scan", "--path", &claude][..], extra].concat();
        let out = r.run_with(&args, &env, "");
        assert_eq!(out.code, 0, "{}", out.json);
        out.json
    };

    let before = r.ok(&["account", "list"]);
    let dry = scan(&[]);
    assert_eq!(
        r.ok(&["account", "list"]),
        before,
        "a scan without --add wrote"
    );
    let at = |p: &str| format!("{home}/{p}");
    let (logins, tokens) = said(&dry);
    assert_eq!(
        logins,
        [
            json!(["claude-code", at(".claude"), "copy", "claude-code"]),
            json!(["codex", at(".codex"), "new", "codex"]),
            json!(["pi", at(".pi/agent"), "copy", "codex"]),
            json!(["kimi", at(".kimi-code"), "new", "kimi"]),
            json!(["grok", at(".grok"), "new", "grok-2"]),
            json!(["claude-code", claude, "new", "claude-code"]),
        ]
    );
    let felix = "mytokens get agent-account-claude-felix --field CLAUDE_CODE_OAUTH_TOKEN";
    assert_eq!(
        tokens,
        [
            json!(["agent-account-claude-felix/default", "new", "claude-felix"]),
            json!(["agent-account-codex/default", "skipped", null]),
        ]
    );
    let felix_token = &dry["tokens"][0];
    assert_eq!(felix_token["token_command"], felix);
    assert_eq!(felix_token["same_as_a_login"], "unknown");
    assert!(dry["tokens"][1]["why"].as_str().unwrap().contains("bound"));
    let l = &dry["logins"];
    assert!(
        l[0]["warning"]
            .as_str()
            .unwrap()
            .contains("refresh each other out")
    );
    assert_eq!(l[1]["identity"], l[2]["identity"]);
    assert_eq!(l[0]["identity"], l[5]["identity"]);
    assert_eq!(l[1]["identity"].as_str().unwrap().len(), 16);
    assert_eq!(l[3]["identity"], Value::Null);
    assert!(
        l[3]["identity_unknown"]
            .as_str()
            .unwrap()
            .contains("no JWT")
    );
    assert_eq!(l[1]["logged_in_at"], "1970-01-01T00:50:00Z");
    assert_eq!(dry["mytokens_unread"], Value::Null);

    let added = scan(&["--add"]);
    let (logins, tokens) = said(&added);
    let statuses: Vec<_> = logins.iter().map(|l| l[2].clone()).collect();
    assert_eq!(
        statuses,
        ["copy", "added", "copy", "added", "added", "added"]
    );
    assert_eq!(
        tokens[0],
        json!([
            "agent-account-claude-felix/default",
            "added",
            "claude-felix"
        ])
    );
    let codex = r.ok(&["account", "show", "codex"]);
    assert_eq!(codex["account"]["login"], "bound");
    assert_eq!(codex["account"]["machine"], "mini");
    assert_eq!(codex["account"]["home"], at(".codex"));
    assert_eq!(codex["account"]["identity"], l[1]["identity"]);
    assert_eq!(codex["logged_in"], true);
    let work = r.ok(&["account", "show", "claude-code"]);
    assert_eq!(work["account"]["home"], claude);
    // (listed, not shown: show runs its token command)
    let accounts = r.ok(&["account", "list"]);
    let portable = accounts["accounts"].as_array().unwrap().iter();
    let portable: Vec<_> = portable.filter(|a| a["name"] == "claude-felix").collect();
    assert_eq!(portable[0]["token_command"], felix);
    assert_eq!(
        r.ok(&["account", "show", "grok"])["account"]["home"],
        "/elsewhere"
    );

    // Again: each is the account it was registered as, nothing more added.
    let list = r.ok(&["account", "list"]);
    let again = scan(&["--add"]);
    assert_eq!(r.ok(&["account", "list"]), list);
    let (logins, tokens) = said(&again);
    let statuses: Vec<_> = logins.iter().map(|l| l[2].clone()).collect();
    assert_eq!(
        statuses,
        [
            "copy",
            "registered",
            "copy",
            "registered",
            "registered",
            "registered"
        ]
    );
    assert_eq!(tokens[0][1], "registered");

    // scan read no secret: mytokens was only listed; no identity or token
    // was said or kept.
    for call in r.argv().lines() {
        assert_eq!(call, "mytokens list");
    }
    let register = std::fs::read(r.dir.join("stablehand.db")).unwrap();
    let shown = format!("{dry}{added}{again}{}", r.stderr());
    for secret in SECRETS {
        assert!(!shown.contains(secret), "{secret} said");
        let kept = register
            .windows(secret.len())
            .any(|w| w == secret.as_bytes());
        assert!(!kept, "{secret} kept");
    }

    // Each engine found answers an account to pick.
    for engine in ["claude-code", "codex", "kimi", "grok"] {
        let pick = r.ok(&["pick", "--engine", engine]);
        assert!(pick["account"].is_string(), "{engine}: {pick}");
    }
    // The portable one's token command prints its token.
    let credential = r.ok(&["credential", "claude-felix", "--json"]);
    assert_eq!(credential["env"]["CLAUDE_CODE_OAUTH_TOKEN"], TOKEN);
}

#[test]
fn a_path_that_is_no_directory_is_refused_and_no_mytokens_is_said() {
    let r = Register::new("scan-refused");
    let home = r.path("empty");
    std::fs::create_dir_all(&home).unwrap();
    let env = [("HOME", home.as_str()), ("PATH", "/usr/bin:/bin")];
    let args = ["account", "scan", "--path", "/no/such/dir", "--add"];
    let why = r.refused_with(&args, &env, "", 1);
    assert!(why.contains("/no/such/dir"), "{why}");
    let out = r.run_with(&["account", "scan"], &env, "");
    assert_eq!(out.code, 0);
    assert_eq!(out.json["logins"], json!([]));
    assert!(
        out.json["mytokens_unread"]
            .as_str()
            .unwrap()
            .contains("not on PATH")
    );
}

#[test]
fn a_kept_home_tells_its_identity_and_what_is_not_the_same_login_is_not_taken_for_it() {
    let r = Register::new("scan-kept");
    let home = r.path("home");
    let codex = r#"{"tokens": {"account_id": "CHATGPT-SECRET"}}"#;
    file(&home, ".codex/auth.json", codex, None);
    // A home of factorium's, kept, holding the same ChatGPT account: read
    // for the account it is, though scan does not look in it for logins.
    let kept = r.path("agent-accounts/chatgpt");
    file(&kept, "auth.json", codex, None);
    let bound = ["--engine", "codex", "--machine", "mini", "--home", &kept];
    r.ok(&[&["account", "add", "chatgpt"][..], &bound].concat());
    // A Claude home of its own with no .claude.json does not borrow the
    // one beside it.
    file(
        &home,
        ".claude.json",
        r#"{"oauthAccount": {"accountUuid": "U", "organizationUuid": "O"}}"#,
        None,
    );
    file(&home, "alone/.credentials.json", "{}", None);
    // A portable account naming another entry's token is not this one's.
    let other = "mytokens get agent-account-other --field CLAUDE_CODE_OAUTH_TOKEN";
    let portable = ["--engine", "claude-code", "--token-command", other];
    r.ok(&[&["account", "add", "other"][..], &portable].concat());
    r.mytokens_entry("agent-account-felix", &[("CLAUDE_CODE_OAUTH_TOKEN", TOKEN)]);

    let env = [("HOME", home.as_str())];
    let alone = format!("{home}/alone");
    let codex_home = format!("{home}/.codex");
    let args = ["account", "scan", "--path", &codex_home, "--path", &alone];
    let out = r.run_with(&args, &env, "").json;
    let (logins, tokens) = said(&out);
    assert_eq!(
        logins,
        [
            json!(["codex", codex_home, "copy", "chatgpt"]),
            json!(["claude-code", alone, "new", "claude-code"]),
        ]
    );
    let unknown = out["logins"][1]["identity_unknown"].as_str().unwrap();
    assert!(unknown.contains(".claude.json"), "{unknown}");
    assert_eq!(
        tokens,
        [json!(["agent-account-felix/default", "new", "felix"])]
    );

    // A mytokens that fails its list is said, and the logins still are.
    let failing = r.path("failing");
    file(
        &failing,
        "mytokens",
        "#!/bin/sh\necho broken >&2\nexit 3\n",
        None,
    );
    let script = Path::new(&failing).join("mytokens");
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{failing}:/usr/bin:/bin");
    let env = [("HOME", home.as_str()), ("PATH", path.as_str())];
    let out = r.run_with(&["account", "scan"], &env, "").json;
    assert!(
        out["mytokens_unread"].as_str().unwrap().contains("failed"),
        "{out}"
    );
    assert_eq!(out["logins"][0]["status"], "copy");
}
