//! `read`, `limit` and `status`, run through the binary against the
//! stand-ins of curl and the engines (tests/contract/fake-*), first on PATH;
//! a portable token is the first line of a file its token command prints.
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The bound login's token, and the portable one its token command prints: stand-ins.
const LOGIN_TOKEN: &str = "sk-ant-oat01-standin-login";
const PORTABLE_TOKEN: &str = "sk-ant-oat01-standin-portable";

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
        for tool in ["curl", "codex", "pi", "agy", "claude"] {
            std::os::unix::fs::symlink(contract.join(format!("fake-{tool}")), bin.join(tool))
                .unwrap();
        }
        // Where the person keeps the portable token: a file of its own.
        std::fs::write(root.join("portable-token"), format!("{PORTABLE_TOKEN}\n")).unwrap();
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
                "FAKE_CODEX_LOG",
                root.join("engines.log").display().to_string(),
            ),
            (
                "FAKE_PI_LOG",
                root.join("engines.log").display().to_string(),
            ),
            (
                "FAKE_CLAUDE_LOG",
                root.join("claude.log").display().to_string(),
            ),
            ("STABLEHAND_MACHINE", "mini".to_string()),
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

    /// The token command that prints the portable token.
    fn token_command(&self) -> String {
        format!("cat {}", self.root.join("portable-token").display())
    }

    /// The portable claude-code account `name`, its token from [`Books::token_command`].
    fn portable(&self, name: &str) {
        let cmd = self.token_command();
        self.ok(&[
            "account",
            "add",
            name,
            "--engine",
            "claude-code",
            "--token-command",
            &cmd,
        ]);
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

    /// The question's answer from now on (fake-claude): a stream of its
    /// rate_limit_event alone, five_hour and seven_day `five` and `seven`
    /// used (0 to 1), resetting in 3 hours and 3 days; when five_hour resets.
    fn question_answers(&self, five: f64, seven: f64) -> String {
        let (five_at, seven_at) = (now() + 3 * 3600, now() + 3 * 86_400);
        let event = json!({ "type": "rate_limit_event", "rate_limit_info": { "unifiedWindows": {
            "five_hour": { "utilization": five, "resetsAt": five_at },
            "seven_day": { "utilization": seven, "resetsAt": seven_at },
        }}});
        let file = self.root.join("stream.jsonl");
        std::fs::write(&file, format!("{event}\n")).unwrap();
        self.set("FAKE_CLAUDE_BODY", &file.display().to_string());
        rfc3339(five_at)
    }

    fn claude_log(&self) -> String {
        std::fs::read_to_string(self.root.join("claude.log")).unwrap_or_default()
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
    // Its usage answered, so no turn was spent.
    assert_eq!(
        (&read["read_by"], &read["spent_turn"]),
        (&json!("usage"), &json!(false))
    );

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
    assert_eq!(status["read_by"], "usage");
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
    // Usage failing, the question is asked instead; it fails too here, so
    // the try fails, its error naming both failures, each with its fix.
    b.set("FAKE_CLAUDE_FAULT", "refused");
    let question = "`claude -p … quota` exited 1: Failed to authenticate. API Error: 401 OAuth access token is invalid.: the token was refused or has expired: `claude auth login` on mini";
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
        assert!(
            error.contains(said) && error.contains(question),
            "{fault}: {error}"
        );
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
            && error.contains("log it in with `claude auth login` on mini"),
        "{error}"
    );
}

#[test]
fn a_portable_login_is_read_with_its_command_s_token_and_a_limit_booked_over_it() {
    let b = Books::new("portable");
    b.portable("claude-felix");
    let read = b.read(&["claude-felix"]);
    assert_eq!(read["outcome"], "read", "{read}");
    // On curl's stdin, never a command line: the curl stand-in logs both.
    assert!(b.log().contains(&format!("Bearer {PORTABLE_TOKEN}")));
    assert!(
        !b.log()
            .lines()
            .any(|l| l.starts_with("--silent") && l.contains(PORTABLE_TOKEN))
    );

    // A caller's limit is booked over the reading.
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

    // A token its command cannot print is a failed try naming the command.
    std::fs::remove_file(b.root.join("portable-token")).unwrap();
    let read = b.read(&["claude-felix", "--now"]);
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains(&format!("`{}` exited 1", b.token_command()))
            && error.contains("stablehand account edit claude-felix --token-command"),
        "{error}"
    );
}

#[test]
fn a_setup_token_refused_usage_is_read_by_one_small_turn_and_its_usage_not_asked_again() {
    let b = Books::new("question");
    std::fs::create_dir(b.root.join("tmp")).unwrap();
    b.set("TMPDIR", &b.root.join("tmp").display().to_string());
    b.portable("claude-jp");
    let resets = b.question_answers(0.07, 0.45);
    // `/api/oauth/usage` refuses a `claude setup-token` token (HTTP 403).
    b.set("FAKE_CURL_FAULT", "scope");
    let read = b.read(&["claude-jp"]);
    assert_eq!(read["outcome"], "read", "{read}");
    assert_eq!(
        (&read["read_by"], &read["spent_turn"]),
        (&json!("question"), &json!(true))
    );
    let five = window(&read["windows"], "five_hour");
    assert_eq!(
        (&five["used_percent"], &five["resets_at"], &five["source"]),
        (&json!(7.0), &json!(resets), &json!("read"))
    );
    assert_eq!(window(&read["windows"], "seven_day")["used_percent"], 45.0);
    assert_eq!(read["windows"].as_array().unwrap().len(), 2, "{read}");

    // Asked of claude by one turn on its smallest model, the token in its
    // environment and on no command line, in an empty config dir of its own.
    let asked = b.claude_log();
    let lines: Vec<&str> = asked.lines().collect();
    assert_eq!(
        lines,
        [
            "-p --model haiku --output-format stream-json --verbose --tools  --strict-mcp-config --no-session-persistence --setting-sources  --system-prompt Answer with one word. quota",
            &format!("token {PORTABLE_TOKEN}"),
            "config empty, run in it",
        ]
    );
    let status = b.status("claude-jp");
    assert_eq!(status["read_by"], "question");
    assert_eq!(window(&status["windows"], "seven_day")["pace"], "steady");

    // Within the hour it waits: no turn is spent.
    assert_eq!(b.read(&["claude-jp"])["outcome"], "waits");
    // With --now it asks again, and says it spent a turn; the usage that
    // refused it is not asked again.
    b.question_answers(0.10, 0.46);
    let forced = b.read(&["claude-jp", "--now"]);
    assert_eq!(
        (&forced["outcome"], &forced["spent_turn"]),
        (&json!("read"), &json!(true))
    );
    assert_eq!(
        window(&forced["windows"], "five_hour")["used_percent"],
        10.0
    );
    assert_eq!(b.log().matches("oauth/usage").count(), 1);
    assert_eq!(b.claude_log().matches(" quota\n").count(), 2);

    // A token the question is refused fails with the fix, the last windows stale.
    b.set("FAKE_CLAUDE_FAULT", "refused");
    let refused = b.read(&["claude-jp", "--now"]);
    let error = refused["error"].as_str().unwrap();
    assert!(
        error.contains("exited 1")
            && error.contains("401 OAuth access token is invalid")
            && error.contains("claude setup-token"),
        "{error}"
    );
    assert_eq!(window(&refused["windows"], "five_hour")["stale"], true);
    // Claude Code not on PATH, or no temporary directory: each fails, saying so.
    std::fs::remove_file(b.root.join("bin/claude")).unwrap();
    let error = b.read(&["claude-jp", "--now"])["error"].clone();
    assert!(
        error.as_str().unwrap().contains("install Claude Code"),
        "{error}"
    );
    b.set("TMPDIR", &b.root.join("no-such-dir").display().to_string());
    let error = b.read(&["claude-jp", "--now"])["error"].clone();
    assert!(error.as_str().unwrap().contains("no-such-dir"), "{error}");
    b.set("TMPDIR", &b.root.join("tmp").display().to_string());

    // Nothing of the questions is left behind: their config dirs are gone.
    let tmp = std::fs::read_dir(b.root.join("tmp")).unwrap().count();
    assert_eq!(
        tmp,
        0,
        "config dirs left in {}",
        b.root.join("tmp").display()
    );
    let db = std::fs::read(b.root.join("config/stablehand.db")).unwrap();
    assert!(!String::from_utf8_lossy(&db).contains(PORTABLE_TOKEN));
}

#[test]
fn usage_failing_otherwise_is_read_by_the_question_for_that_read_only() {
    let b = Books::new("fallback");
    b.portable("felix");
    let recorded = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/contract/recorded/claude-code-usage-429.http")
        .display()
        .to_string();
    // A 5xx, and a 403 for another reason than the scope: each the
    // answer's status line, a blank line and its body.
    let answer = |name: &str, status: u16, error: &str| {
        let file = b.root.join(name);
        let body = json!({ "type": "error", "error": { "type": error, "message": "Stand-in" } });
        std::fs::write(&file, format!("HTTP/2 {status} \n\n{body}\n")).unwrap();
        file.display().to_string()
    };
    let overloaded = answer("overloaded.http", 529, "overloaded_error");
    let forbidden = answer("forbidden.http", 403, "permission_error");
    let mut asked = 0;
    for (key, value) in [
        ("FAKE_CURL_RECORDED", recorded.as_str()),
        ("FAKE_CURL_RECORDED", overloaded.as_str()),
        ("FAKE_CURL_RECORDED", forbidden.as_str()),
        ("FAKE_CURL_FAULT", "unreachable"),
    ] {
        b.question_answers(0.06, 0.55);
        b.set(key, value);
        let read = b.read(&["felix", "--now"]);
        assert_eq!(
            (&read["outcome"], &read["read_by"], &read["spent_turn"]),
            (&json!("read"), &json!("question"), &json!(true)),
            "{value}: {read}"
        );
        assert_eq!(window(&read["windows"], "five_hour")["used_percent"], 6.0);
        assert_eq!(window(&read["windows"], "seven_day")["used_percent"], 55.0);
        asked += 1;
        assert_eq!(b.log().matches("oauth/usage").count(), asked);
        // The next read asks usage first again, and books its windows.
        b.set(key, "");
        let read = b.read(&["felix", "--now"]);
        assert_eq!(
            (&read["read_by"], &read["spent_turn"]),
            (&json!("usage"), &json!(false)),
            "{value}: {read}"
        );
        assert_eq!(window(&read["windows"], "five_hour")["used_percent"], 22.0);
        asked += 1;
        assert_eq!(b.log().matches("oauth/usage").count(), asked);
    }
    assert_eq!(b.claude_log().matches(" quota\n").count(), 4);

    // Both failing, the try fails, naming the 429 and the question's refusal.
    b.set("FAKE_CURL_RECORDED", &recorded);
    b.set("FAKE_CLAUDE_FAULT", "refused");
    let read = b.read(&["felix", "--now"]);
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("answered HTTP 429: Rate limited. Please try again later.: try again later")
            && error.contains("401 OAuth access token is invalid")
            && error.contains("stablehand account edit felix"),
        "{error}"
    );
}

#[test]
fn read_without_a_name_reads_every_account_and_each_outcome_is_said() {
    let b = Books::new("all");
    b.set("STABLEHAND_MACHINE", "mini");
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
        "agy-air",
        "--engine",
        "agy",
        "--machine",
        "air",
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
    assert_eq!(outcomes, [("agy-air", "failed"), ("claude-mini", "read")]);
    let status = b.ok(&["status"]);
    let agy = &status["accounts"][0];
    assert_eq!(
        (&agy["name"], &agy["windows"]),
        (&json!("agy-air"), &json!([]))
    );
    assert!(agy["tried_at"].is_string(), "{agy}");
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

/// `pick`'s answer: the account picked, or the reasons, in order, each
/// account was passed over.
fn picked(b: &Books, args: &[&str]) -> Result<String, Vec<(String, String)>> {
    let answer = b.ok(&[&["pick", "--engine", "claude-code"], args].concat());
    if let Some(name) = answer["account"].as_str() {
        return Ok(name.to_string());
    }
    assert_eq!(answer["none"], true, "{answer}");
    Err(reasons(&answer))
}

/// Each account a none answer passed over, with its reason, in order.
fn reasons(answer: &Value) -> Vec<(String, String)> {
    answer["why"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let s = |k: &str| p[k].as_str().unwrap().to_string();
            (s("account"), s("reason"))
        })
        .collect()
}

#[test]
fn pick_takes_the_engine_s_usable_accounts_and_passes_over_each_by_guild_s_rules() {
    let b = Books::new("pick");
    for name in ["claude-felix", "claude-off", "claude-two"] {
        b.portable(name);
    }
    for args in [
        &["claude-gone", "--engine", "claude-code"][..],
        &[
            "claude-mini",
            "--engine",
            "claude-code",
            "--machine",
            "mini",
        ],
        &["chatgpt", "--engine", "codex"],
    ] {
        b.ok(&[&["account", "add"], args].concat());
    }
    b.ok(&["off", "claude-off", "--reason", "card declined"]);
    // claude-felix read 22% and 52% used; claude-two never read: it comes after.
    b.read(&["claude-felix"]);
    assert_eq!(
        picked(&b, &["--machine", "studio"]),
        Ok("claude-felix".into())
    );

    // No count of sessions running passes an account over by itself: its
    // windows and their pace decide (stablehand#26). claude-felix's pace,
    // shared by three, runs out; claude-two, never read, runs on at no rate.
    let many = [
        "--machine",
        "studio",
        "--running",
        "claude-felix=2,claude-two=40",
    ];
    assert_eq!(picked(&b, &many), Ok("claude-two".into()));
    let one_more = ["--machine", "studio", "--running", "claude-two=40"];
    assert_eq!(picked(&b, &one_more), Ok("claude-felix".into()));

    // claude-two off, claude-felix at two running is passed over by its pace.
    b.ok(&["off", "claude-two", "--reason", "paused"]);
    let two = ["--machine", "studio", "--running", "claude-felix=2"];
    assert!(
        picked(&b, &two)
            .unwrap_err()
            .contains(&("claude-felix".into(), "runs_out".into()))
    );

    // A limit on it: none, until the limit lifts.
    let lifts = rfc3339(now() + 1800);
    b.ok(&[
        "limit",
        "claude-felix",
        "--window",
        "five_hour",
        "--resets-at",
        &lifts,
    ]);
    let answer = b.ok(&[
        "pick",
        "--engine",
        "claude-code",
        "--machine",
        "studio",
        "--running",
        "claude-two=2",
    ]);
    assert_eq!(
        (&answer["none"], &answer["until"]),
        (&json!(true), &json!(lifts)),
        "{answer}"
    );
    let why = reasons(&answer);
    let why: Vec<(&str, &str)> = why.iter().map(|(a, r)| (a.as_str(), r.as_str())).collect();
    assert_eq!(
        why,
        [
            ("claude-felix", "limit"),
            ("claude-gone", "not_logged_in"),
            ("claude-mini", "elsewhere"),
            ("claude-off", "off"),
            ("claude-two", "off"),
        ]
    );
    assert_eq!(answer["why"][3]["why"], "switched off: card declined");
    // On its machine, the bound account not logged in is said so.
    let on_mini = picked(&b, &["--machine", "mini", "--running", "claude-two=2"]);
    assert!(
        on_mini
            .unwrap_err()
            .contains(&("claude-mini".into(), "not_logged_in".into())),
    );
}

#[test]
fn pick_stops_an_account_at_its_stop_percent_and_one_whose_window_would_run_out() {
    let b = Books::new("pick-stop");
    b.portable("claude-felix");
    // 95% of seven_day used: stopped, until it resets.
    b.answers(10.0, 95.0);
    b.read(&["claude-felix"]);
    let answer = b.ok(&["pick", "--engine", "claude-code", "--machine", "studio"]);
    assert_eq!(answer["why"][0]["reason"], "stopped", "{answer}");
    assert_eq!(answer["until"], answer["why"][0]["until"]);
    let why = answer["why"][0]["why"].as_str().unwrap();
    assert!(
        why.contains("seven_day is 95% used, at its stop of 90%"),
        "{why}"
    );
    // 40% used, under half: no rate runs it out; a stop of 40 stops it.
    b.answers(10.0, 40.0);
    b.read(&["claude-felix", "--now"]);
    assert_eq!(
        picked(&b, &["--machine", "studio"]),
        Ok("claude-felix".into())
    );
    let lower = ["--machine", "studio", "--stop-percent", "40"];
    assert_eq!(
        picked(&b, &lower),
        Err(vec![("claude-felix".into(), "stopped".into())])
    );

    // 60% of five_hour used two hours in: 150% at its reset at that rate.
    b.answers(60.0, 10.0);
    b.read(&["claude-felix", "--now"]);
    assert_eq!(
        picked(&b, &["--machine", "studio"]),
        Err(vec![("claude-felix".into(), "runs_out".into())])
    );
}

#[test]
fn pick_refuses_a_count_it_cannot_read_and_an_account_not_kept() {
    let b = Books::new("pick-refusals");
    b.portable("claude-felix");
    for (args, exit, part) in [
        (vec!["--running", "nobody=1"], 1, "no account `nobody`"),
        (
            vec!["--running", "claude-felix"],
            2,
            "give it as <account>=<n>",
        ),
        (
            vec!["--running", "claude-felix=two"],
            2,
            "give it as <account>=<n>",
        ),
        (vec!["--stop-percent", "0"], 2, "--stop-percent"),
    ] {
        let (code, json) = b.run(&[&["pick", "--engine", "claude-code"], &args[..]].concat());
        assert_eq!(code, exit, "{args:?}: {json}");
        let error = json["error"].as_str().unwrap();
        assert!(error.contains(part), "{args:?}: {error}");
    }
    let (code, json) = b.run(&["pick", "--engine", "nope"]);
    assert_eq!(code, 2, "{json}");
    // An engine with no account here: none, and said.
    let answer = b.ok(&["pick", "--engine", "codex"]);
    assert_eq!(
        (&answer["none"], &answer["until"], walked(&answer)),
        (
            &json!(true),
            &Value::Null,
            vec![step("codex", "-", "-", "no_account")]
        )
    );
}

/// The pi stand-in's bearer (tests/contract/fake-pi) and its login's id.
const PI_BEARER: &str = "eyJstand-in.chatgpt.bearer";
const PI_ACCOUNT_ID: &str = "acct-standin";

impl Books {
    /// A bound ChatGPT account `name` of `engine` on mini, its home made:
    /// pi's with its openai-codex login's id in its auth.json.
    fn chatgpt(&self, name: &str, engine: &str) -> String {
        let home = self.root.join(name).display().to_string();
        std::fs::create_dir_all(&home).unwrap();
        let auth = json!({ "openai-codex": { "type": "oauth", "accountId": PI_ACCOUNT_ID } });
        std::fs::write(format!("{home}/auth.json"), auth.to_string()).unwrap();
        self.ok(&[
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
        home
    }

    /// The ChatGPT account's answer from now on, codex's and wham's: primary
    /// and secondary this much used, resetting in 3 hours and 3 days; when
    /// primary resets, in seconds.
    fn chatgpt_answers(&self, primary: f64, secondary: f64) -> u64 {
        let (p, s) = (now() + 3 * 3600, now() + 3 * 86_400);
        let codex = json!({
            "limitId": "codex",
            "primary": { "usedPercent": primary, "windowDurationMins": 300, "resetsAt": p },
            "secondary": { "usedPercent": secondary, "windowDurationMins": 10_080, "resetsAt": s },
        });
        let wham = json!({ "rate_limit": {
            "primary_window": { "used_percent": primary, "limit_window_seconds": 18_000, "reset_at": p },
            "secondary_window": { "used_percent": secondary, "limit_window_seconds": 604_800, "reset_at": s },
        }});
        for (var, file, body) in [
            ("FAKE_CODEX_BODY", "codex.json", codex),
            ("FAKE_CURL_WHAM_BODY", "wham.json", wham),
        ] {
            let file = self.root.join(file);
            std::fs::write(&file, body.to_string()).unwrap();
            self.set(var, &file.display().to_string());
        }
        p
    }

    fn engines_log(&self) -> String {
        std::fs::read_to_string(self.root.join("engines.log")).unwrap_or_default()
    }
}

#[test]
fn a_chatgpt_account_is_read_through_codex_s_app_server_and_pi_s_login() {
    let b = Books::new("chatgpt");
    for (name, engine, used, asked) in [
        ("chatgpt", "codex", 42.0, "codex app-server CODEX_HOME="),
        (
            "chatgpt-pi",
            "pi",
            12.0,
            "pi auth print-bearer-token --provider openai-codex PI_CODING_AGENT_DIR=",
        ),
    ] {
        let resets = b.chatgpt_answers(used, 9.0);
        let home = b.chatgpt(name, engine);
        let read = b.read(&[name]);
        assert_eq!(read["outcome"], "read", "{read}");
        let primary = window(&read["windows"], "primary");
        assert_eq!(
            (
                &primary["used_percent"],
                &primary["resets_at"],
                &primary["group"]
            ),
            (&json!(used), &json!(rfc3339(resets)), &Value::Null)
        );
        assert_eq!(window(&read["windows"], "secondary")["used_percent"], 9.0);
        assert_eq!(read["windows"].as_array().unwrap().len(), 2, "{read}");
        assert!(
            b.engines_log().ends_with(&format!("{asked}{home}\n")),
            "{}",
            b.engines_log()
        );
        let status = b.status(name);
        assert_eq!(
            (&status["engine"], &status["pace"]),
            (&json!(engine), &json!("steady"))
        );
    }
    // pi's bearer and the login's id go to curl on its stdin, never in its arguments.
    let log = b.log();
    let (args, config) = log.split_once('\n').unwrap();
    assert!(
        args.ends_with("https://chatgpt.com/backend-api/wham/usage") && !args.contains(PI_BEARER),
        "{args}"
    );
    assert!(
        config.contains(&format!("header = \"Authorization: Bearer {PI_BEARER}\""))
            && config.contains(&format!("header = \"ChatGPT-Account-Id: {PI_ACCOUNT_ID}\"")),
        "{config}"
    );
}

#[test]
fn a_chatgpt_read_refused_leaves_the_last_windows_stale_and_is_not_retried_in_the_hour() {
    let b = Books::new("chatgpt-refused");
    b.chatgpt_answers(42.0, 9.0);
    b.chatgpt("chatgpt", "codex");
    b.chatgpt("chatgpt-pi", "pi");
    b.read(&["chatgpt"]);
    b.read(&["chatgpt-pi"]);
    for (name, fault, value, said) in [
        (
            "chatgpt",
            "FAKE_CODEX_FAULT",
            "logged-out",
            "codex account authentication required",
        ),
        (
            "chatgpt-pi",
            "FAKE_CURL_FAULT",
            "unauthorized",
            "HTTP 401: Could not parse your authentication token",
        ),
        (
            "chatgpt-pi",
            "FAKE_PI_FAULT",
            "not-logged-in",
            "is not configured with an OAuth bearer token",
        ),
    ] {
        b.set(fault, value);
        let read = b.read(&[name, "--now"]);
        assert_eq!(read["outcome"], "failed", "{read}");
        let error = read["error"].as_str().unwrap();
        assert!(
            error.contains(said)
                && error.contains("log it in with `")
                && error.contains("` on mini")
                && !error.contains(PI_BEARER),
            "{error}"
        );
        let primary = window(&read["windows"], "primary");
        assert_eq!(
            (&primary["stale"], &primary["source"]),
            (&json!(true), &json!("read"))
        );
        assert_eq!(b.status(name)["failed"], read["error"]);
        // The refused credential is not asked again before the hour.
        let asked = (b.engines_log(), b.log());
        assert_eq!(b.read(&[name])["outcome"], "waits");
        assert_eq!((b.engines_log(), b.log()), asked);
        b.set(fault, "");
    }

    // A portable ChatGPT account, or one bound to another machine, is not read here.
    b.ok(&["account", "add", "codex-portable", "--engine", "codex"]);
    b.ok(&[
        "account",
        "add",
        "pi-far",
        "--engine",
        "pi",
        "--machine",
        "studio",
    ]);
    let asked = b.engines_log();
    for (name, said) in [
        ("codex-portable", "bind account `codex-portable`"),
        ("pi-far", "bound to studio and this machine is mini"),
    ] {
        let read = b.read(&[name]);
        assert_eq!(read["outcome"], "failed", "{read}");
        assert!(read["error"].as_str().unwrap().contains(said), "{read}");
    }
    assert_eq!(b.engines_log(), asked);
}

/// A grok login in `file`, its bearer `key`, expiring `expires_at`, as grok
/// leaves `auth.json`.
fn grok_login(file: &Path, key: &str, expires_at: u64) {
    let entry =
        json!({ "key": key, "expires_at": rfc3339(expires_at), "refresh_token": "stand-in" });
    std::fs::write(file, json!({ "https://auth.x.ai::b1": entry }).to_string()).unwrap();
}

#[test]
fn grok_s_credits_are_booked_its_weekly_window_and_one_per_product() {
    const GROK_TOKEN: &str = "eyJ0eX-standin-grok";
    let b = Books::new("grok");
    let home = b.root.join("grok-home");
    std::fs::create_dir_all(&home).unwrap();
    let auth = home.join("auth.json");
    grok_login(&auth, GROK_TOKEN, now() + 3 * 3600);
    std::fs::write(home.join("version.json"), r#"{"version":"1.0.92"}"#).unwrap();
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/engines/fake-engine");
    std::os::unix::fs::symlink(engine, b.root.join("bin/grok")).unwrap();
    let file = |name: &str| b.root.join(name).display().to_string();
    b.set("FAKE_ARGV_LOG", &file("argv.log"));
    let resets = rfc3339(now() + 86_400);
    let body = json!({ "config": {
        "currentPeriod": { "type": "USAGE_PERIOD_TYPE_WEEKLY", "start": rfc3339(now() - 6 * 86_400), "end": resets },
        "creditUsagePercent": 85.0,
        "productUsage": [{ "product": "Api", "usagePercent": 81.0 }, { "product": "GrokBuild", "usagePercent": 4.0 }],
    }});
    std::fs::write(file("grok.json"), body.to_string()).unwrap();
    b.set("FAKE_CURL_BODY", &file("grok.json"));
    let h = home.display().to_string();
    let add = ["account", "add", "grok-mini", "--engine", "grok"];
    b.ok(&[&add[..], &["--machine", "mini", "--home", &h]].concat());

    let read = b.read(&["grok-mini"]);
    assert_eq!(read["outcome"], "read", "{read}");
    assert_eq!(read["windows"].as_array().unwrap().len(), 3, "{read}");
    let build = &read["windows"][2];
    assert_eq!(
        (
            &build["name"],
            &build["group"],
            &build["used_percent"],
            &build["resets_at"]
        ),
        (
            &json!("weekly"),
            &json!("GrokBuild"),
            &json!(4.0),
            &json!(resets)
        )
    );
    let log = b.log();
    let (args, config) = log.split_once('\n').unwrap();
    assert!(
        args.ends_with("https://cli-chat-proxy.grok.com/v1/billing?format=credits"),
        "{args}"
    );
    assert!(!args.contains(GROK_TOKEN), "{args}");
    assert!(
        config.contains(&format!("Authorization: Bearer {GROK_TOKEN}\"")),
        "{config}"
    );
    assert!(
        config.contains("x-grok-client-version: 1.0.92\""),
        "{config}"
    );
    // Six days in, 85% runs to 99%: steady. GrokBuild, 4%, would be spare
    // were a share of the credits paced on its own.
    let status = b.status("grok-mini");
    assert_eq!(
        (&status["engine"], &status["pace"]),
        (&json!("grok"), &json!("steady"))
    );

    // A refused bearer is a failed try: the windows kept, stale; not asked
    // again within the hour.
    b.set("FAKE_CURL_FAULT", "unauthorized");
    let read = b.read(&["grok-mini", "--now"]);
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("HTTP 401: Invalid or expired credentials")
            && error.contains(&format!(
                "log it in again with `GROK_HOME={h} grok login` on mini"
            )),
        "{error}"
    );
    assert!(!error.contains(GROK_TOKEN), "{error}");
    assert_eq!(read["windows"][0]["stale"], true);
    assert_eq!(b.read(&["grok-mini"])["outcome"], "waits");
    assert_eq!(b.log().matches("v1/billing").count(), 2);

    // A login expiring within the minute is grok's to refresh, in the
    // account's home; one it leaves so is a failed try that asks nothing.
    b.set("FAKE_CURL_FAULT", "");
    grok_login(&auth, GROK_TOKEN, now() + 30);
    let read = b.read(&["grok-mini", "--now"]);
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("has expired and `grok models` did not refresh it"),
        "{error}"
    );
    let argv = std::fs::read_to_string(file("argv.log")).unwrap();
    assert_eq!(argv, format!("grok models GROK_HOME={h}\n"));
    assert_eq!(b.log().matches("v1/billing").count(), 2);
    // One grok renews is read with its new bearer.
    grok_login(
        &b.root.join("renewed.json"),
        "eyJ0eX-renewed",
        now() + 3 * 3600,
    );
    b.set("FAKE_ENGINE_LOGIN", &file("renewed.json"));
    assert_eq!(b.read(&["grok-mini", "--now"])["outcome"], "read");
    assert!(
        b.log()
            .ends_with("Bearer eyJ0eX-renewed\"\nheader = \"x-grok-client-version: 1.0.92\"\n")
    );

    // A portable grok account is told to bind.
    b.ok(&["account", "add", "grok-felix", "--engine", "grok"]);
    let read = b.read(&["grok-felix"]);
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("stablehand account edit grok-felix --machine"),
        "{error}"
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
            { "name": "Gemini Models", "description": "Models within this group: Gemini 4 Neon, Orchestrator (Neon)", "buckets": [
                bucket("gemini-weekly", "weekly", 0.75, rfc3339(now() + 4 * 86_400)),
                bucket("gemini-5h", "5h", 0.0, rfc3339(now() + 2 * 3600)),
            ]},
            { "name": "Claude and GPT models", "description": "Models within this group: Claude Opus 5.5, GPT-OSS 120B", "buckets": [
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
    assert_eq!(
        log,
        format!("-p /usage --output-format json HOME={home}\nmodels HOME={home}\n")
    );
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
            && error.contains(&format!("log it in with `HOME={home} agy` on mini")),
        "{error}"
    );
    let gemini = window(&read["windows"], "gemini-5h");
    assert_eq!(
        (&gemini["used_percent"], &gemini["stale"]),
        (&json!(100.0), &json!(true))
    );
    assert_eq!(b.read(&["agy-mini"])["outcome"], "waits");
    // A failed `/usage` asks for no model list.
    assert_eq!(
        std::fs::read_to_string(b.root.join("agy.log"))
            .unwrap()
            .lines()
            .count(),
        3
    );
}

/// `pick --engine <engine>`'s answer with `args` after it.
fn pick_of(b: &Books, engine: &str, args: &[&str]) -> Value {
    b.ok(&[&["pick", "--engine", engine], args].concat())
}

#[test]
fn pick_of_a_model_judges_an_agy_account_by_its_group_s_windows() {
    let b = Books::new("agy-model");
    let home = b.agy_home();
    let add = ["account", "add", "agy-mini", "--engine", "agy"];
    b.ok(&[&add[..], &["--machine", "mini", "--home", &home]].concat());
    b.read(&["agy-mini"]);
    // status shows each group's models, by the ids `agy models` gives.
    let status = b.status("agy-mini");
    assert_eq!(
        (
            &window(&status["windows"], "gemini-5h")["models"],
            &window(&status["windows"], "3p-5h")["models"],
        ),
        (
            &json!([
                "gemini-4-neon-high",
                "gemini-4-neon-low",
                "gemini-pro-orchestrator-neon"
            ]),
            &json!(["claude-opus-5-5-high", "gpt-oss-120b-medium"])
        )
    );
    // The Gemini group used up: a Claude session may start on it, a Gemini
    // one not, until its 5h window resets.
    assert_eq!(
        pick_of(&b, "agy", &["--model", "claude-opus-5-5-high"]),
        json!({"engine": "agy", "model": "claude-opus-5-5-high", "account": "agy-mini", "why": [], "model_in_no_group": []})
    );
    let gemini = pick_of(&b, "agy", &["--model", "gemini-4-neon-high"]);
    assert_eq!(
        (reasons(&gemini), &gemini["model_in_no_group"]),
        (vec![("agy-mini".into(), "stopped".into())], &json!([]))
    );
    assert!(
        gemini["why"][0]["why"]
            .as_str()
            .unwrap()
            .contains("gemini-5h"),
        "{gemini}"
    );
    // A model no group holds: judged by every window, and said.
    let unknown = pick_of(&b, "agy", &["--model", "gpt-9"]);
    assert_eq!(
        (reasons(&unknown), &unknown["model_in_no_group"]),
        (
            vec![("agy-mini".into(), "stopped".into())],
            &json!(["agy-mini"])
        )
    );
    // Without --model, as before: the account is judged whole, no model said.
    let whole = pick_of(&b, "agy", &[]);
    assert_eq!(reasons(&whole), [("agy-mini".into(), "stopped".into())]);
    assert_eq!(whole["model"], Value::Null, "{whole}");
    // `agy models` failing, the windows book no model: every model is in
    // no group, and the account is judged whole.
    b.set("FAKE_AGY_MODELS", "fails");
    b.read(&["agy-mini", "--now"]);
    assert_eq!(
        window(&b.status("agy-mini")["windows"], "3p-5h").get("models"),
        Some(&json!([]))
    );
    let blind = pick_of(&b, "agy", &["--model", "claude-opus-5-5-high"]);
    assert_eq!(
        (reasons(&blind), &blind["model_in_no_group"]),
        (
            vec![("agy-mini".into(), "stopped".into())],
            &json!(["agy-mini"])
        )
    );
    // An empty --model is refused.
    let (code, empty) = b.run(&["pick", "--engine", "agy", "--model", ""]);
    assert_eq!(code, 2, "{empty}");
}

#[test]
fn pick_of_a_model_leaves_an_engine_with_no_groups_as_it_was() {
    let b = Books::new("model-claude");
    b.portable("claude-felix");
    b.read(&["claude-felix"]);
    let args = ["--model", "claude-opus-5-5", "--stop-percent", "50"];
    let with = pick_of(&b, "claude-code", &args);
    let without = pick_of(&b, "claude-code", &args[2..]);
    assert_eq!(
        (&with["why"][0]["why"], &with["model_in_no_group"]),
        (&without["why"][0]["why"], &json!([]))
    );
    assert_eq!(reasons(&with), [("claude-felix".into(), "stopped".into())]);
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

/// Each (engine, model, account, reason) the answer says it passed over, in order.
fn walked(answer: &Value) -> Vec<(String, String, String, String)> {
    let s = |p: &Value, k: &str| p[k].as_str().unwrap_or("-").to_string();
    answer["why"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                s(p, "engine"),
                s(p, "model"),
                s(p, "account"),
                s(p, "reason"),
            )
        })
        .collect()
}

fn step(
    engine: &str,
    model: &str,
    account: &str,
    reason: &str,
) -> (String, String, String, String) {
    (engine.into(), model.into(), account.into(), reason.into())
}

/// The host's order (factorium's kit): claude-code on opus, codex, grok, and
/// agy on Gemini, then on Claude.
const KIT: [&str; 6] = [
    "order",
    "set",
    "claude-code@opus",
    "codex",
    "grok",
    "agy@gemini-4-neon-high,claude-opus-5-5-high",
];

#[test]
fn pick_walks_the_order_and_a_used_up_gemini_group_falls_to_agy_s_opus() {
    let b = Books::new("order-walk");
    let home = b.agy_home();
    let add = ["account", "add", "agy-mini", "--engine", "agy"];
    b.ok(&[&add[..], &["--machine", "mini", "--home", &home]].concat());
    b.read(&["agy-mini"]);
    // claude-felix's five_hour 95% used: stopped until it resets.
    b.portable("claude-felix");
    let five_resets = b.answers(95.0, 10.0);
    b.read(&["claude-felix"]);

    // The agy ids are checked against `agy models`, run in agy-mini's home.
    let set = b.ok(&KIT);
    let order = json!({ "order": [
        { "engine": "claude-code", "models": ["opus"] },
        { "engine": "codex", "models": [] },
        { "engine": "grok", "models": [] },
        { "engine": "agy", "models": ["gemini-4-neon-high", "claude-opus-5-5-high"] },
    ]});
    assert_eq!(set, order);
    assert_eq!(b.ok(&["order", "show"]), order);
    let log = std::fs::read_to_string(b.root.join("agy.log")).unwrap();
    assert!(log.ends_with(&format!("models HOME={home}\n")), "{log}");

    // The walk: claude-code stopped, codex and grok kept no account, agy's
    // Gemini group used up, so agy on Opus.
    let pick = b.ok(&["pick"]);
    assert_eq!(
        (&pick["engine"], &pick["model"], &pick["account"]),
        (
            &json!("agy"),
            &json!("claude-opus-5-5-high"),
            &json!("agy-mini")
        ),
        "{pick}"
    );
    assert_eq!(
        walked(&pick),
        [
            step("claude-code", "opus", "claude-felix", "stopped"),
            step("codex", "-", "-", "no_account"),
            step("grok", "-", "-", "no_account"),
            step("agy", "gemini-4-neon-high", "agy-mini", "stopped"),
        ]
    );
    let skipped = pick["why"][1]["why"].as_str().unwrap();
    assert!(skipped.contains("no codex account is kept"), "{skipped}");

    // --engine and --model narrow the walk, in the order's order.
    let gemini = b.ok(&["pick", "--engine", "agy", "--model", "gemini-4-neon-high"]);
    assert_eq!(
        (&gemini["none"], walked(&gemini)),
        (
            &json!(true),
            vec![step("agy", "gemini-4-neon-high", "agy-mini", "stopped")]
        )
    );
    let opus = b.ok(&["pick", "--model", "claude-opus-5-5-high"]);
    assert_eq!(
        (&opus["engine"], &opus["account"], walked(&opus)),
        (&json!("agy"), &json!("agy-mini"), vec![])
    );
    let two = b.ok(&["pick", "--engine", "codex", "--engine", "claude-code"]);
    assert_eq!(
        (&two["until"], walked(&two)),
        (
            &json!(five_resets),
            vec![
                step("claude-code", "opus", "claude-felix", "stopped"),
                step("codex", "-", "-", "no_account"),
            ]
        )
    );
    // An engine or model not in the order is tried after the rest.
    let kimi = b.ok(&["pick", "--engine", "kimi", "--engine", "claude-code"]);
    assert_eq!(
        walked(&kimi),
        [
            step("claude-code", "opus", "claude-felix", "stopped"),
            step("kimi", "-", "-", "no_account"),
        ]
    );
    let gpt = b.ok(&["pick", "--engine", "agy", "--model", "gpt-oss-120b-medium"]);
    assert_eq!(
        (&gpt["model"], &gpt["account"]),
        (&json!("gpt-oss-120b-medium"), &json!("agy-mini"))
    );

    // agy's Claude group limited for an hour too: none, until the earliest
    // of the three lifts.
    let lifts = rfc3339(now() + 3600);
    let limit = ["limit", "agy-mini", "--window", "3p-5h", "--resets-at"];
    b.ok(&[&limit[..], &[&lifts]].concat());
    let none = b.ok(&["pick"]);
    assert_eq!(
        (&none["none"], &none["until"], walked(&none).len()),
        (&json!(true), &json!(lifts), 5),
        "{none}"
    );
}

#[test]
fn order_set_refuses_what_stablehand_does_not_know_and_writes_nothing() {
    let b = Books::new("order-refusals");
    let home = b.agy_home();
    let add = ["account", "add", "agy-mini", "--engine", "agy"];
    b.ok(&[&add[..], &["--machine", "mini", "--home", &home]].concat());
    // `agy models` runs in the agy account's home, not that of another
    // engine's account bound here, nor of an agy account bound elsewhere.
    let elsewhere = ["--machine", "air", "--home", "/nowhere"];
    b.ok(&[
        &["account", "add", "agy-air", "--engine", "agy"][..],
        &elsewhere,
    ]
    .concat());
    b.chatgpt("a-codex", "codex");
    // No order and no --engine: nothing to walk.
    let (code, none) = b.run(&["pick"]);
    assert_eq!((code, &none["code"]), (1, &json!("no_order")), "{none}");
    assert!(
        none["error"]
            .as_str()
            .unwrap()
            .contains("stablehand order set")
    );
    b.ok(&["order", "set", "codex"]);
    for (args, part) in [
        (vec!["nope"], "no engine `nope`"),
        (vec!["agy@gemini-9"], "`agy models` lists no `gemini-9`"),
        (vec!["codex", "codex@gpt"], "`codex` is given twice"),
        (vec!["agy@"], "`agy@` names no model"),
        (
            vec!["agy@claude-opus-5-5-high,claude-opus-5-5-high"],
            "twice",
        ),
    ] {
        let (code, json) = b.run(&[&["order", "set"], &args[..]].concat());
        assert_eq!(
            (code, &json["code"]),
            (1, &json!("bad_order")),
            "{args:?}: {json}"
        );
        let error = json["error"].as_str().unwrap();
        assert!(error.contains(part), "{args:?}: {error}");
    }
    // `agy models` failing, an agy model cannot be checked: refused, the fix said.
    b.set("FAKE_AGY_MODELS", "fails");
    let (code, json) = b.run(&["order", "set", "agy@claude-opus-5-5-high"]);
    assert_eq!(code, 1, "{json}");
    assert!(
        json["error"].as_str().unwrap().contains("agy models"),
        "{json}"
    );
    assert_eq!(
        b.ok(&["order", "show"]),
        json!({ "order": [{ "engine": "codex", "models": [] }] })
    );
    // --model naming no model of the order, and no --engine: nothing to walk.
    let (code, json) = b.run(&["pick", "--model", "opus"]);
    assert_eq!((code, &json["code"]), (1, &json!("no_order")), "{json}");
}

impl Books {
    /// `statusline args…` on `stdin`: its line, and how long it took.
    fn statusline(&self, args: &[&str], stdin: &str) -> (String, Duration) {
        use std::io::Write;
        let started = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_stablehand"))
            .args([&["statusline"][..], args].concat())
            .env_clear()
            .envs(std::env::vars().filter(|(k, _)| k == "LLVM_PROFILE_FILE"))
            .envs(self.env.borrow().iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        (String::from_utf8(out.stdout).unwrap(), started.elapsed())
    }

    /// The calls curl's stand-in was given.
    fn calls(&self) -> usize {
        self.log()
            .lines()
            .filter(|l| l.starts_with("--silent"))
            .count()
    }

    /// Waits up to 10s for `done`.
    fn until(&self, what: &str, done: impl Fn() -> bool) {
        let by = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < by, "never: {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn the_status_line_starts_one_read_where_its_books_are_stale_and_waits_on_none() {
    let b = Books::new("statusline-reads");
    b.portable("claude-nate");
    let hold = b.root.join("hold");
    std::fs::write(&hold, "").unwrap();
    b.set("FAKE_CURL_HOLD", &hold.display().to_string());
    let args = ["--account", "claude-nate"];

    // No books and no rate_limits: one read starts, and the line is the
    // books as they are, at once, the read still held running.
    let (line, took) = b.statusline(&args, "{}");
    assert_eq!(line, "claude-nate\n");
    assert!(took < Duration::from_secs(5), "{took:?}");
    b.until("the read asks curl", || b.calls() == 1);
    // A second refresh while it runs starts none.
    let (line, _) = b.statusline(&args, "{}");
    assert_eq!(line, "claude-nate\n");
    std::fs::remove_file(&hold).unwrap();
    b.until("the reading is booked", || {
        b.statusline(&args, "{}").0 == "claude-nate · 5h 22% · 7d 52%\n"
    });
    // Fresh books start none.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(b.calls(), 1, "{}", b.log());

    // With rate_limits on stdin nothing starts, though no books were kept.
    b.portable("claude-other");
    let given = include_str!("statusline/rate-limits.json");
    let (line, _) = b.statusline(&["--account", "claude-other"], given);
    assert_eq!(line, "claude-other · 5h 24% · 7d 41%\n");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(b.calls(), 1, "{}", b.log());
}
