//! `account scan`, through the binary, over a fixture home tree and the
//! stand-ins of optokens and curl.
mod support;

use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};
use support::Register;

/// Every identity and token the fixture's login files and optokens hold:
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

/// A register whose PATH has curl's stand-in (tests/contract/fake-curl) too.
fn register(name: &str) -> Register {
    let r = Register::new(name);
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contract/fake-curl");
    std::os::unix::fs::symlink(fake, r.path("bin/curl")).unwrap();
    r
}

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
    r.optokens_entry(
        "agent-account-claude-felix",
        &[
            ("HARNESS", "claude-code"),
            ("CLAUDE_CODE_OAUTH_TOKEN", TOKEN),
        ],
    );
    r.optokens_entry(
        "agent-account-codex",
        &[("HARNESS", "codex"), ("MACHINE", "mini")],
    );
    r.optokens_entry("huggingface", &[("HF_TOKEN", "hf-not-an-account")]);
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
    let r = register("scan");
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
    let felix = "optokens get agent-account-claude-felix --field CLAUDE_CODE_OAUTH_TOKEN";
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
    assert_eq!(dry["optokens_unread"], Value::Null);

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

    // scan read no secret but the token, to ask its organization: optokens
    // was listed and asked that token alone; no identity or token was said
    // or kept.
    for call in r.argv().lines() {
        assert!(call == "optokens list" || call == felix, "{call}");
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
fn a_path_that_is_no_directory_is_refused_and_no_optokens_is_said() {
    let r = register("scan-refused");
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
        out.json["optokens_unread"]
            .as_str()
            .unwrap()
            .contains("not on PATH")
    );
    // Its help names each default home it looks in.
    let (code, help) = r.raw(&["account", "scan", "--help"], &[], "");
    assert_eq!(code, 0);
    assert!(
        help.contains("pi `~/.pi/agent`") && help.contains("agy `~`"),
        "{help}"
    );
}

#[test]
fn a_kept_home_tells_its_identity_and_what_is_not_the_same_login_is_not_taken_for_it() {
    let r = register("scan-kept");
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
    let other = "optokens get agent-account-other --field CLAUDE_CODE_OAUTH_TOKEN";
    let portable = ["--engine", "claude-code", "--token-command", other];
    r.ok(&[&["account", "add", "other"][..], &portable].concat());
    r.optokens_entry("agent-account-felix", &[("CLAUDE_CODE_OAUTH_TOKEN", TOKEN)]);
    // An entry with neither a token nor a MACHINE is no bound one.
    r.optokens_entry("agent-account-notes", &[("HARNESS", "claude-code")]);

    // An account bound to another machine is not this home's, whatever
    // its home path holds here.
    let grok = r#"{"https://auth.x.ai::e1": {"user_id": "G"}}"#;
    file(&home, ".grok/auth.json", grok, None);
    let far = r.path("far");
    file(&far, "auth.json", grok, None);
    let remote = ["--engine", "grok", "--machine", "studio", "--home", &far];
    r.ok(&[&["account", "add", "far"][..], &remote].concat());

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
            json!(["grok", format!("{home}/.grok"), "new", "grok"]),
            json!(["claude-code", alone, "new", "claude-code"]),
        ]
    );
    let unknown = out["logins"][2]["identity_unknown"].as_str().unwrap();
    assert!(unknown.contains(".claude.json"), "{unknown}");
    assert_eq!(
        tokens,
        [
            json!(["agent-account-felix/default", "new", "felix"]),
            json!(["agent-account-notes/default", "skipped", null]),
        ]
    );
    let why = out["tokens"][1]["why"].as_str().unwrap();
    assert_eq!(why, "it holds no engine's token field");

    // An optokens that fails its list is said with the fault it answered on
    // stdout, and the logins still are.
    let failing = r.path("failing");
    file(
        &failing,
        "optokens",
        "#!/bin/sh\necho '{\"error\":[{\"fault\":\"no token to open the vault\",\"fix\":\"run optokens login\",\"kind\":\"no_token\"}]}'\nexit 2\n",
        None,
    );
    let script = Path::new(&failing).join("optokens");
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{failing}:/usr/bin:/bin");
    let env = [("HOME", home.as_str()), ("PATH", path.as_str())];
    let out = r.run_with(&["account", "scan"], &env, "").json;
    assert_eq!(
        out["optokens_unread"],
        "`optokens list` failed: no token to open the vault (run optokens login)",
        "{out}"
    );
    assert_eq!(out["logins"][0]["status"], "copy");

    // One that cannot start is said so, not taken for one not on PATH.
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    let out = r.run_with(&["account", "scan"], &env, "").json;
    let why = out["optokens_unread"].as_str().unwrap();
    assert!(why.contains("could not start"), "{why}");
}

/// A register made before tokens told their organization, as on Felix's
/// host: a portable `claude-code` token of the Claude Max subscription a
/// bound `claude-code-2` (~/.claude) is logged in to.
#[test]
fn a_token_of_a_login_s_personal_organization_is_its_copy_and_list_says_so() {
    let r = register("scan-org");
    let home = r.path("home");
    let team = r.path("team-claude");
    let login = r#"{"claudeAiOauth": {"refreshToken": "sk-ant-ort01-refresh-SECRET"}}"#;
    let max = r#"{"oauthAccount": {"accountUuid": "UUID-FELIX-SECRET", "organizationUuid": "ORG-MAX-SECRET", "organizationType": "claude_max"}}"#;
    file(&home, ".claude/.credentials.json", login, Some(1_000));
    file(&home, ".claude.json", max, None);
    let theplant = r#"{"oauthAccount": {"accountUuid": "UUID-WORK-SECRET", "organizationUuid": "ORG-TEAM-SECRET", "organizationType": "claude_team"}}"#;
    file(&team, ".credentials.json", login, Some(2_000));
    file(&team, ".claude.json", theplant, None);
    let max_token = "sk-ant-oat01-org-ORG-MAX-SECRET";
    let team_token = "sk-ant-oat01-org-ORG-TEAM-SECRET";
    let cmd = r.kept_token("agent-account-claude-code", max_token);
    r.ok(&[
        "account",
        "add",
        "claude-code",
        "--engine",
        "claude-code",
        "--token-command",
        &cmd,
    ]);
    let claude = format!("{home}/.claude");
    // The machine the register's runs are on (tests/support).
    let here = "mini";
    let bound = [
        "--engine",
        "claude-code",
        "--machine",
        here,
        "--home",
        &claude,
    ];
    r.ok(&[&["account", "add", "claude-code-2"][..], &bound].concat());
    r.optokens_entry(
        "agent-account-felix",
        &[("CLAUDE_CODE_OAUTH_TOKEN", max_token)],
    );
    r.optokens_entry(
        "agent-account-theplant",
        &[("CLAUDE_CODE_OAUTH_TOKEN", team_token)],
    );
    r.optokens_entry(
        "agent-account-theplant-jp",
        &[("CLAUDE_CODE_OAUTH_TOKEN", team_token)],
    );
    r.optokens_entry(
        "agent-account-other",
        &[("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat01-somewhere-SECRET")],
    );

    let copy_of = || {
        let list = r.ok(&["account", "list"]);
        let all = list["accounts"].as_array().unwrap().clone();
        all.iter()
            .map(|a| json!([a["name"], a["copy_of"]]))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        copy_of(),
        [json!(["claude-code", null]), json!(["claude-code-2", null])]
    );

    let env = [("HOME", home.as_str())];
    let scan = |extra: &[&str]| {
        let args = [&["account", "scan", "--path", &team][..], extra].concat();
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
    let tokens: Vec<_> = dry["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| json!([t["entry"], t["status"], t["account"], t["same_as_a_login"]]))
        .collect();
    assert_eq!(
        tokens,
        [
            json!([
                "agent-account-claude-code/default",
                "registered",
                "claude-code",
                "claude-code-2"
            ]),
            json!([
                "agent-account-felix/default",
                "copy",
                "claude-code-2",
                "claude-code-2"
            ]),
            json!(["agent-account-other/default", "new", "other", "unknown"]),
            json!([
                "agent-account-theplant-jp/default",
                "new",
                "theplant-jp",
                "unknown"
            ]),
            json!([
                "agent-account-theplant/default",
                "new",
                "theplant",
                "unknown"
            ]),
        ]
    );
    let t = &dry["tokens"];
    let warning = |i: usize| t[i]["warning"].as_str().unwrap_or_default().to_string();
    assert!(warning(0).contains("one of them only"), "{}", warning(0));
    assert!(
        warning(1).contains("claude-code-2's subscription"),
        "{}",
        warning(1)
    );
    assert_eq!(t[2]["warning"], Value::Null);
    assert!(
        warning(3).contains("agent-account-theplant/default") && warning(3).contains("team"),
        "{}",
        warning(3)
    );
    assert!(
        warning(4).contains("agent-account-theplant-jp/default"),
        "{}",
        warning(4)
    );
    assert_eq!(t[0]["identity"], t[1]["identity"]);
    assert_eq!(t[3]["identity"], t[4]["identity"]);
    assert_ne!(t[0]["identity"], t[2]["identity"]);

    // A token refused its organization is not known, and is said why.
    let refused = [("HOME", home.as_str()), ("FAKE_CURL_FAULT", "unauthorized")];
    let out = r.run_with(&["account", "scan"], &refused, "").json;
    let why = out["tokens"][0]["identity_unknown"].as_str().unwrap();
    assert!(why.contains("HTTP 401") && why.contains("refused"), "{why}");
    assert_eq!(out["tokens"][0]["same_as_a_login"], "unknown");
    // One answered naming no organization says its answer changed, and how to check.
    let unnamed = [("HOME", home.as_str()), ("FAKE_CURL_FAULT", "unnamed")];
    let out = r.run_with(&["account", "scan"], &unnamed, "").json;
    let why = out["tokens"][0]["identity_unknown"].as_str().unwrap();
    assert!(
        why.contains("HTTP 200 naming no organization")
            && why.contains("truefit contract --real claude-code-organization"),
        "{why}"
    );

    // --add registers no copy, and keeps the hashes the old register lacked.
    let added = scan(&["--add"]);
    let statuses: Vec<_> = added["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["status"].clone())
        .collect();
    assert_eq!(statuses, ["registered", "copy", "added", "added", "added"]);
    assert_eq!(r.run(&["account", "show", "felix"]).code, 1);
    let all = copy_of();
    assert!(
        all.contains(&json!(["claude-code", "claude-code-2"])),
        "{all:?}"
    );
    assert!(all.contains(&json!(["claude-code-2", null])), "{all:?}");
    assert!(all.contains(&json!(["theplant", null])), "{all:?}");
    assert!(all.contains(&json!(["theplant-jp", null])), "{all:?}");
    let shown = r.ok(&["account", "show", "claude-code"]);
    assert_eq!(shown["account"]["copy_of"], "claude-code-2");
    let bound = r.ok(&["account", "show", "claude-code-2"]);
    assert_eq!(bound["account"]["organization_type"], "claude_max");
    assert_eq!(bound["account"]["organization"], t[0]["identity"]);
    let status = r.ok(&["status", "claude-code"]);
    assert_eq!(status["accounts"][0]["copy_of"], "claude-code-2");

    // pick passes over the copy while its account is on here
    // (stablehand#32); accounts of another or no known identity are
    // judged as before.
    let pick = |on: &str| r.ok(&["pick", "--engine", "claude-code", "--machine", on]);
    assert_eq!(pick(here)["account"], "claude-code-2");
    let lifts = "2099-01-01T00:00:00Z";
    let limit = ["limit", "claude-code-2", "--resets-at", lifts];
    r.ok(&limit);
    for name in ["claude-code-3", "other", "theplant", "theplant-jp"] {
        r.ok(&["off", name, "--reason", "a test"]);
    }
    let none = pick(here);
    let why: Vec<_> = none["why"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| json!([p["account"], p["reason"]]))
        .collect();
    assert_eq!(
        why,
        [
            json!(["claude-code", "copy"]),
            json!(["claude-code-2", "limit"]),
            json!(["claude-code-3", "off"]),
            json!(["other", "off"]),
            json!(["theplant", "off"]),
            json!(["theplant-jp", "off"]),
        ],
        "{none}"
    );
    let copy = none["why"][0]["why"].as_str().unwrap();
    assert!(copy.contains("a copy of claude-code-2"), "{copy}");
    r.ok(&["on", "other"]);
    assert_eq!(pick(here)["account"], "other");
    r.ok(&["off", "other", "--reason", "a test"]);
    // Its account bound to another machine, or off, the copy is picked.
    assert_eq!(pick("elsewhere")["account"], "claude-code");
    r.ok(&["off", "claude-code-2", "--reason", "a copy's account"]);
    assert_eq!(pick(here)["account"], "claude-code");

    let register = std::fs::read(r.dir.join("stablehand.db")).unwrap();
    let said = format!("{dry}{added}{}", r.stderr());
    for secret in [
        "ORG-MAX-SECRET",
        "ORG-TEAM-SECRET",
        "UUID-FELIX-SECRET",
        "UUID-WORK-SECRET",
        "somewhere-SECRET",
        "00000000-0000-4000-8000-000000000000",
    ] {
        assert!(!said.contains(secret), "{secret} said");
        assert!(
            !register
                .windows(secret.len())
                .any(|w| w == secret.as_bytes()),
            "{secret} kept"
        );
    }
}
