//! `read`, `limit` and `status`, run through the binary against the
//! stand-ins of curl and sesame (tests/contract/fake-*), first on PATH.
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The bound login's token, and the portable one sesame hands over: stand-ins.
const LOGIN_TOKEN: &str = "sk-ant-oat01-standin-login";
const SESAME_TOKEN: &str = "sk-ant-oat01-standin-portable";

struct Books {
    root: PathBuf,
    env: RefCell<Vec<(String, String)>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// `secs` as UTC RFC 3339, worked out apart from stablehand's own.
fn rfc3339(secs: u64) -> String {
    let out = Command::new("date")
        .args(["-u", "-r", &secs.to_string(), "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

impl Books {
    fn new(name: &str) -> Books {
        let root = std::env::temp_dir()
            .join(format!("stablehand-usage-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let contract = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contract");
        for tool in ["curl", "sesame", "agy"] {
            std::os::unix::fs::symlink(contract.join(format!("fake-{tool}")), bin.join(tool))
                .unwrap();
        }
        // The sesame stand-in's store: claude-felix logged in, as `account login` leaves it.
        let sesame = root.join("sesame/fake");
        for kept in ["providers", "tokens"] {
            std::fs::create_dir_all(sesame.join(kept)).unwrap();
        }
        std::fs::write(sesame.join("providers/stablehand-claude-felix"), "").unwrap();
        std::fs::write(sesame.join("tokens/stablehand-claude-felix"), SESAME_TOKEN).unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(
            home.join(".claude/.credentials.json"),
            json!({ "claudeAiOauth": { "accessToken": LOGIN_TOKEN } }).to_string(),
        )
        .unwrap();
        let env = [
            ("PATH", format!("{}:/usr/bin:/bin", bin.display())),
            ("HOME", home.display().to_string()),
            (
                "STABLEHAND_CONFIG_DIR",
                root.join("config").display().to_string(),
            ),
            ("FAKE_CURL_LOG", root.join("curl.log").display().to_string()),
            (
                "SESAME_CONFIG_DIR",
                root.join("sesame").display().to_string(),
            ),
        ];
        let books = Books {
            env: RefCell::new(env.into_iter().map(|(k, v)| (k.to_string(), v)).collect()),
            root,
        };
        books.answers(22.0, 52.0);
        books
    }

    /// The endpoint's answer from now on: five_hour and seven_day this
    /// much used, resetting in 3 hours and 3 days; when five_hour resets.
    fn answers(&self, five: f64, seven: f64) -> String {
        let five_resets = rfc3339(now() + 3 * 3600);
        let body = json!({
            "five_hour": { "utilization": five, "resets_at": five_resets, "limit_dollars": null },
            "seven_day": { "utilization": seven, "resets_at": rfc3339(now() + 3 * 86_400) },
            "seven_day_opus": null,
            "extra_usage": { "is_enabled": false },
        });
        let file = self.root.join("body.json");
        std::fs::write(&file, body.to_string()).unwrap();
        self.set("FAKE_CURL_BODY", &file.display().to_string());
        five_resets
    }

    fn set(&self, key: &str, value: &str) {
        let mut env = self.env.borrow_mut();
        env.retain(|(k, _)| k != key);
        env.push((key.into(), value.into()));
    }

    fn run(&self, args: &[&str]) -> (i32, Value) {
        let out = Command::new(env!("CARGO_BIN_EXE_stablehand"))
            .args(args)
            .env_clear()
            // The coverage run's profile, so the binary's lines are counted.
            .envs(std::env::vars().filter(|(k, _)| k == "LLVM_PROFILE_FILE"))
            .envs(self.env.borrow().iter().map(|(k, v)| (k, v)))
            .env("USER", "tester")
            .output()
            .unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1, "one JSON line: {stdout}");
        (
            out.status.code().unwrap(),
            serde_json::from_str(&stdout).unwrap(),
        )
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (code, json) = self.run(args);
        assert_eq!(code, 0, "{args:?}: {json}");
        json
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.root.join("curl.log")).unwrap_or_default()
    }

    /// The one read of `read`'s answer.
    fn read(&self, args: &[&str]) -> Value {
        let answer = self.ok(&[&["read"], args].concat());
        let reads = answer["reads"].as_array().unwrap();
        assert_eq!(reads.len(), 1, "{answer}");
        reads[0].clone()
    }

    fn status(&self, name: &str) -> Value {
        self.ok(&["status", name])["accounts"][0].clone()
    }
}

fn window<'a>(windows: &'a Value, name: &str) -> &'a Value {
    windows
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == name)
        .unwrap_or_else(|| panic!("no {name} in {windows}"))
}

#[test]
fn read_books_a_bound_login_s_windows_once_an_hour_unless_now() {
    let b = Books::new("bound");
    let resets = b.answers(22.0, 52.0);
    b.ok(&[
        "account",
        "add",
        "claude-mini",
        "--engine",
        "claude-code",
        "--machine",
        "mini",
    ]);
    let read = b.read(&["claude-mini"]);
    assert_eq!(
        (&read["account"], &read["outcome"]),
        (&json!("claude-mini"), &json!("read"))
    );
    let five = window(&read["windows"], "five_hour");
    assert_eq!(five["used_percent"], 22.0);
    assert_eq!(five["resets_at"], resets);
    assert_eq!(
        (&five["source"], &five["stale"], &five["group"]),
        (&json!("read"), &json!(false), &Value::Null)
    );
    assert_eq!(window(&read["windows"], "seven_day")["used_percent"], 52.0);
    assert_eq!(read["windows"].as_array().unwrap().len(), 2, "{read}");

    // The bearer and the beta header go to curl on its stdin, never in its arguments.
    let log = b.log();
    let (args, config) = log.split_once('\n').unwrap();
    assert!(
        args.ends_with("https://api.anthropic.com/api/oauth/usage"),
        "{args}"
    );
    assert!(!args.contains(LOGIN_TOKEN), "{args}");
    assert!(
        config.contains(&format!("header = \"Authorization: Bearer {LOGIN_TOKEN}\"")),
        "{config}"
    );
    assert!(
        config.contains("header = \"anthropic-beta: oauth-2025-04-20\""),
        "{config}"
    );

    // Read again within the hour: it waits, and asks nothing.
    b.answers(30.0, 53.0);
    let again = b.read(&["claude-mini"]);
    assert_eq!(again["outcome"], "waits");
    assert!(
        again["next_at"].as_str().unwrap() > rfc3339(now() + 3500).as_str(),
        "{again}"
    );
    assert_eq!(window(&again["windows"], "five_hour")["used_percent"], 22.0);
    assert_eq!(b.log().matches("oauth/usage").count(), 1);

    let forced = b.read(&["claude-mini", "--now"]);
    assert_eq!(forced["outcome"], "read");
    assert_eq!(
        window(&forced["windows"], "five_hour")["used_percent"],
        30.0
    );

    let status = b.status("claude-mini");
    assert_eq!(
        (&status["name"], &status["engine"], &status["off"]),
        (&json!("claude-mini"), &json!("claude-code"), &json!(false))
    );
    assert_eq!(status["pace"], "steady");
    assert_eq!(status["failed"], Value::Null);
    assert_eq!(status["read_at"], status["tried_at"]);
    assert!(status["read_next"].is_string(), "{status}");
    assert_eq!(window(&status["windows"], "seven_day")["pace"], "steady");
}

#[test]
fn a_reader_s_failure_is_booked_and_said_and_the_last_windows_kept_stale() {
    let b = Books::new("failed");
    b.ok(&[
        "account",
        "add",
        "claude-mini",
        "--engine",
        "claude-code",
        "--machine",
        "mini",
    ]);
    b.read(&["claude-mini"]);
    for (fault, said) in [
        (
            "unauthorized",
            "HTTP 401: Invalid bearer token: the login was refused or has expired",
        ),
        ("unreachable", "Could not resolve host: api.anthropic.com"),
    ] {
        b.set("FAKE_CURL_FAULT", fault);
        let read = b.read(&["claude-mini", "--now"]);
        assert_eq!(read["outcome"], "failed", "{read}");
        let error = read["error"].as_str().unwrap();
        assert!(error.contains(said), "{fault}: {error}");
        assert!(!error.contains(LOGIN_TOKEN), "{error}");
        let five = window(&read["windows"], "five_hour");
        assert_eq!(
            (&five["used_percent"], &five["stale"]),
            (&json!(22.0), &json!(true))
        );
        let status = b.status("claude-mini");
        assert_eq!(status["failed"], read["error"]);
        assert_ne!(status["read_at"], Value::Null);
    }
    // A failed try counts as a try: within the hour, it waits.
    b.set("FAKE_CURL_FAULT", "");
    assert_eq!(b.read(&["claude-mini"])["outcome"], "waits");
    // A login file missing is a failed try, naming its fix.
    std::fs::remove_file(b.root.join("home/.claude/.credentials.json")).unwrap();
    let read = b.read(&["claude-mini", "--now"]);
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains(".credentials.json")
            && error.contains("stablehand account login claude-mini"),
        "{error}"
    );
}

#[test]
fn a_portable_login_is_read_with_sesame_s_token_and_a_setup_token_gets_a_limit() {
    let b = Books::new("portable");
    b.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    let read = b.read(&["claude-felix"]);
    assert_eq!(read["outcome"], "read", "{read}");
    assert!(b.log().contains(&format!("Bearer {SESAME_TOKEN}")));

    // A `claude setup-token` token is given no usage: the fix is a limit.
    b.set("FAKE_CURL_FAULT", "scope");
    let read = b.read(&["claude-felix", "--now"]);
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("HTTP 403") && error.contains("stablehand limit claude-felix"),
        "{error}"
    );

    let lifts = rfc3339(now() + 1800);
    let limit = b.ok(&[
        "limit",
        "claude-felix",
        "--window",
        "five_hour",
        "--resets-at",
        &lifts,
    ]);
    assert_eq!(limit["limit"]["account"], "claude-felix");
    assert_eq!(
        (&limit["limit"]["window"], &limit["limit"]["resets_at"]),
        (&json!("five_hour"), &json!(lifts))
    );
    let status = b.status("claude-felix");
    assert_eq!(status["pace"], "used_up");
    assert_eq!(status["limit"]["window"], "five_hour");
    let five = window(&status["windows"], "five_hour");
    assert_eq!(
        (
            &five["used_percent"],
            &five["source"],
            &five["pace"],
            &five["resets_at"]
        ),
        (
            &json!(100.0),
            &json!("limit"),
            &json!("used_up"),
            &json!(lifts)
        )
    );
    // The window the limit does not name stays as read.
    assert_eq!(window(&status["windows"], "seven_day")["source"], "read");

    // A token sesame does not keep is a failed try naming the login.
    std::fs::remove_file(b.root.join("sesame/fake/tokens/stablehand-claude-felix")).unwrap();
    let read = b.read(&["claude-felix", "--now"]);
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("sesame keeps no token")
            && error.contains("stablehand account login claude-felix"),
        "{error}"
    );
}

#[test]
fn read_without_a_name_reads_every_account_and_one_with_no_reader_is_said() {
    let b = Books::new("all");
    b.ok(&[
        "account",
        "add",
        "claude-mini",
        "--engine",
        "claude-code",
        "--machine",
        "mini",
    ]);
    b.ok(&[
        "account",
        "add",
        "chatgpt",
        "--engine",
        "codex",
        "--machine",
        "mini",
    ]);
    let answer = b.ok(&["read"]);
    let outcomes: Vec<_> = answer["reads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["account"].as_str().unwrap(),
                r["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        outcomes,
        [("chatgpt", "no_reader"), ("claude-mini", "read")]
    );
    let status = b.ok(&["status"]);
    let chatgpt = &status["accounts"][0];
    assert_eq!(
        (&chatgpt["name"], &chatgpt["windows"], &chatgpt["tried_at"]),
        (&json!("chatgpt"), &json!([]), &Value::Null)
    );
    assert_eq!(status["accounts"].as_array().unwrap().len(), 2);
}

#[test]
fn a_limit_without_a_window_uses_up_the_account_until_the_next_reading() {
    let b = Books::new("limit");
    b.ok(&[
        "account",
        "add",
        "claude-mini",
        "--engine",
        "claude-code",
        "--machine",
        "mini",
    ]);
    let limit = b.ok(&["limit", "claude-mini"]);
    assert_eq!(
        (&limit["limit"]["window"], &limit["limit"]["resets_at"]),
        (&Value::Null, &Value::Null)
    );
    assert_eq!(b.status("claude-mini")["pace"], "used_up");
    b.read(&["claude-mini"]);
    let status = b.status("claude-mini");
    assert_eq!(
        (&status["pace"], &status["limit"]),
        (&json!("steady"), &Value::Null)
    );
}

#[test]
fn limit_refuses_a_time_it_cannot_read_or_one_past_and_books_nothing() {
    let b = Books::new("refusals");
    b.ok(&[
        "account",
        "add",
        "claude-mini",
        "--engine",
        "claude-code",
        "--machine",
        "mini",
    ]);
    let before = b.status("claude-mini");
    let past = rfc3339(now() - 60);
    for (args, part) in [
        (
            vec!["limit", "claude-mini", "--resets-at", "tomorrow"],
            "RFC 3339",
        ),
        (
            vec!["limit", "claude-mini", "--resets-at", &past],
            "is past",
        ),
        (
            vec!["limit", "claude-mini", "--window", " "],
            "--window is empty",
        ),
        (vec!["limit", "nobody"], "stablehand account add nobody"),
        (vec!["status", "nobody"], "no account `nobody`"),
        (vec!["read", "nobody"], "no account `nobody`"),
    ] {
        let (code, json) = b.run(&args);
        assert_eq!(code, 1, "{args:?}: {json}");
        let error = json["error"].as_str().unwrap();
        assert!(error.contains(part), "{args:?}: {error}");
    }
    assert_eq!(b.status("claude-mini"), before);
}

#[test]
fn an_account_removed_takes_its_books_with_it() {
    let b = Books::new("rm");
    b.ok(&[
        "account",
        "add",
        "claude-mini",
        "--engine",
        "claude-code",
        "--machine",
        "mini",
    ]);
    b.read(&["claude-mini"]);
    b.ok(&["account", "rm", "claude-mini"]);
    b.ok(&[
        "account",
        "add",
        "claude-mini",
        "--engine",
        "claude-code",
        "--machine",
        "mini",
    ]);
    let status = b.status("claude-mini");
    assert_eq!(
        (&status["windows"], &status["read_at"]),
        (&json!([]), &Value::Null)
    );
}

impl Books {
    /// An agy home signed in (its login file a stand-in), answering its
    /// Gemini group's 5h window used up, resetting in 2 hours, and the
    /// other group's not yet used.
    /// This machine is mini, and agy logs each call.
    fn agy_here(&self) {
        self.set(
            "FAKE_AGY_LOG",
            &self.root.join("agy.log").display().to_string(),
        );
        self.set("STABLEHAND_MACHINE", "mini");
    }

    fn agy_home(&self) -> String {
        let home = self.root.join("agy-home");
        let login = home.join(".gemini/antigravity-cli");
        std::fs::create_dir_all(&login).unwrap();
        std::fs::write(login.join("antigravity-oauth-token"), "stand-in").unwrap();
        let bucket = |id: &str, window: &str, left: f64, reset: String| json!({ "id": id, "window": window, "remaining_fraction": left, "reset_time": reset });
        let body = json!({ "status": "SUCCESS", "num_turns": 0, "command": { "name": "usage", "data": { "groups": [
            { "name": "Gemini Models", "buckets": [
                bucket("gemini-weekly", "weekly", 0.75, rfc3339(now() + 4 * 86_400)),
                bucket("gemini-5h", "5h", 0.0, rfc3339(now() + 2 * 3600)),
            ]},
            { "name": "Claude and GPT models", "buckets": [
                bucket("3p-5h", "5h", 1.0, "1970-01-01T00:00:00Z".into()),
            ]},
        ]}}});
        let file = self.root.join("agy.json");
        std::fs::write(&file, body.to_string()).unwrap();
        self.set("FAKE_AGY_BODY", &file.display().to_string());
        self.set(
            "FAKE_AGY_LOG",
            &self.root.join("agy.log").display().to_string(),
        );
        self.set("STABLEHAND_MACHINE", "mini");
        home.display().to_string()
    }
}

#[test]
fn an_agy_account_is_read_in_its_home_a_window_per_model_group() {
    let b = Books::new("agy");
    let home = b.agy_home();
    b.ok(&[
        "account",
        "add",
        "agy-mini",
        "--engine",
        "agy",
        "--machine",
        "mini",
        "--home",
        &home,
    ]);
    let read = b.read(&["agy-mini"]);
    assert_eq!(read["outcome"], "read", "{read}");
    let log = std::fs::read_to_string(b.root.join("agy.log")).unwrap();
    assert_eq!(log, format!("-p /usage --output-format json HOME={home}\n"));
    let gemini = window(&read["windows"], "gemini-5h");
    assert_eq!(
        (&gemini["group"], &gemini["used_percent"], &gemini["pace"]),
        (&json!("Gemini Models"), &json!(100.0), &json!("used_up"))
    );
    assert_eq!(
        window(&read["windows"], "gemini-weekly")["used_percent"],
        25.0
    );
    // A reset of 1970 is a window not yet used: none used, no reset.
    let unused = window(&read["windows"], "3p-5h");
    assert_eq!(
        (
            &unused["group"],
            &unused["used_percent"],
            &unused["resets_at"]
        ),
        (&json!("Claude and GPT models"), &json!(0.0), &Value::Null)
    );
    assert_eq!(b.status("agy-mini")["pace"], "used_up");

    // Signed out, the read fails, says how to sign in, and keeps the last
    // windows, stale; within the hour it is not tried again.
    std::fs::remove_file(format!(
        "{home}/.gemini/antigravity-cli/antigravity-oauth-token"
    ))
    .unwrap();
    let read = b.read(&["agy-mini", "--now"]);
    assert_eq!(read["outcome"], "failed", "{read}");
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("authentication failed")
            && error.contains("stablehand account login agy-mini"),
        "{error}"
    );
    let gemini = window(&read["windows"], "gemini-5h");
    assert_eq!(
        (&gemini["used_percent"], &gemini["stale"]),
        (&json!(100.0), &json!(true))
    );
    assert_eq!(b.read(&["agy-mini"])["outcome"], "waits");
    assert_eq!(
        std::fs::read_to_string(b.root.join("agy.log"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn an_agy_account_portable_or_bound_elsewhere_is_not_run() {
    let b = Books::new("agy-elsewhere");
    b.agy_here();
    b.ok(&["account", "add", "agy-portable", "--engine", "agy"]);
    b.ok(&[
        "account",
        "add",
        "agy-air",
        "--engine",
        "agy",
        "--machine",
        "air",
    ]);
    let error = |name: &str| b.read(&[name])["error"].as_str().unwrap().to_string();
    let portable = error("agy-portable");
    assert!(
        portable.contains("stablehand account edit agy-portable --machine"),
        "{portable}"
    );
    let elsewhere = error("agy-air");
    assert!(
        elsewhere.contains("bound to air and this machine is mini: read it on air"),
        "{elsewhere}"
    );
    assert!(!b.root.join("agy.log").exists());
    // Bound here with no home, agy runs in the user's own.
    b.ok(&[
        "account",
        "add",
        "agy-own",
        "--engine",
        "agy",
        "--machine",
        "mini",
    ]);
    b.read(&["agy-own"]);
    let log = std::fs::read_to_string(b.root.join("agy.log")).unwrap();
    assert!(
        log.ends_with(&format!("HOME={}\n", b.root.join("home").display())),
        "{log}"
    );
}
