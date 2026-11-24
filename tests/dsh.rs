//! dsh's reader end to end: `read` and `status` of a DeepSeek account, a
//! balance and no window, run through the binary against the stand-ins of
//! curl (tests/contract/fake-curl-dsh), sesame and dsh's login, first on PATH.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The grant dsh keeps in a bound login's home, and the token sesame hands
/// over for a portable one: stand-ins.
const GRANT: &str = "dsh-standin-grant";
const SESAME_TOKEN: &str = "dsh-standin-portable";

struct Dsh {
    root: PathBuf,
    env: Vec<(String, String)>,
}

impl Dsh {
    fn new(name: &str) -> Dsh {
        let root = std::env::temp_dir()
            .join(format!("stablehand-dsh-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        for (to, name) in [
            ("contract/fake-curl-dsh", "curl"),
            ("contract/fake-sesame", "sesame"),
            ("engines/fake-engine", "dsh"),
        ] {
            std::os::unix::fs::symlink(tests.join(to), bin.join(name)).unwrap();
        }
        // dsh's credential store in the bound account's home, as `dsh` leaves it.
        let home = root.join("dsh-home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".credentials.yaml"),
            format!(
                "version: 1\nrecords:\n  deepseek-account-platform/device:\n    kind: grant\n    payload:\n      id: 448a2b16\n  deepseek-account-platform/default:\n    kind: grant\n    payload:\n      version: 1\n      token: {GRANT}\n      issuer: https://platform.deepseek.com\n"
            ),
        )
        .unwrap();
        let sesame = root.join("sesame/fake");
        for kept in ["providers", "tokens"] {
            std::fs::create_dir_all(sesame.join(kept)).unwrap();
        }
        std::fs::write(sesame.join("providers/stablehand-deepseek-p"), "").unwrap();
        std::fs::write(sesame.join("tokens/stablehand-deepseek-p"), SESAME_TOKEN).unwrap();
        let at = |p: &str| root.join(p).display().to_string();
        let env = vec![
            ("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())),
            ("HOME".into(), at("home")),
            ("STABLEHAND_CONFIG_DIR".into(), at("config")),
            ("STABLEHAND_MACHINE".into(), "mini".into()),
            ("FAKE_CURL_LOG".into(), at("curl.log")),
            ("FAKE_ARGV_LOG".into(), at("argv.log")),
            ("SESAME_CONFIG_DIR".into(), at("sesame")),
        ];
        Dsh { root, env }
    }

    fn home(&self) -> String {
        self.root.join("dsh-home").display().to_string()
    }

    fn set(&mut self, key: &str, value: &str) {
        self.env.retain(|(k, _)| k != key);
        self.env.push((key.into(), value.into()));
    }

    fn ok(&self, args: &[&str]) -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_stablehand"))
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .env("USER", "tester")
            .output()
            .unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1, "one JSON line: {stdout}");
        assert_eq!(out.status.code(), Some(0), "{args:?}: {stdout}");
        serde_json::from_str(&stdout).unwrap()
    }

    /// The one read of `read`'s answer.
    fn read(&self, args: &[&str]) -> Value {
        let answer = self.ok(&[&["read"], args].concat());
        assert_eq!(answer["reads"].as_array().unwrap().len(), 1, "{answer}");
        answer["reads"][0].clone()
    }

    fn status(&self, name: &str) -> Value {
        self.ok(&["status", name])["accounts"][0].clone()
    }

    fn asks(&self) -> usize {
        std::fs::read_to_string(self.root.join("curl.log"))
            .unwrap_or_default()
            .matches("get_user_summary")
            .count()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.root.join("curl.log")).unwrap_or_default()
    }
}

/// The wallets of an answer, without when they were booked.
fn wallets(balances: &Value) -> Vec<(String, String, f64, bool)> {
    balances
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["wallet"].as_str().unwrap().to_string(),
                b["currency"].as_str().unwrap().to_string(),
                b["amount"].as_f64().unwrap(),
                b["stale"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn cny(normal: f64, stale: bool) -> Vec<(String, String, f64, bool)> {
    vec![
        ("normal".into(), "CNY".into(), normal, stale),
        ("bonus".into(), "CNY".into(), 0.0, stale),
    ]
}

#[test]
fn read_books_a_bound_login_s_balance_and_status_shows_it() {
    let d = Dsh::new("bound");
    let home = d.home();
    d.ok(&[
        "account",
        "add",
        "deepseek",
        "--engine",
        "dsh",
        "--machine",
        "mini",
        "--home",
        &home,
    ]);
    let read = d.read(&["deepseek"]);
    assert_eq!(
        (&read["outcome"], &read["windows"]),
        (&json!("read"), &json!([]))
    );
    assert_eq!(wallets(&read["balances"]), cny(94.67002426, false));

    // The grant and dsh's client headers go to curl on its stdin, never in its arguments.
    let log = d.log();
    let (args, config) = log.split_once('\n').unwrap();
    assert!(
        args.ends_with("https://platform.deepseek.com/api/v0/users/get_user_summary"),
        "{args}"
    );
    assert!(!args.contains(GRANT), "{args}");
    for header in [
        format!("x-dsh-auth-token: {GRANT}"),
        "x-client-bundle-id;".into(),
        "x-client-platform: web".into(),
        "x-client-locale: en_US".into(),
    ] {
        assert!(
            config.contains(&format!("header = \"{header}\"")),
            "{config}"
        );
    }

    let status = d.status("deepseek");
    assert_eq!(
        (&status["engine"], &status["pace"], &status["windows"]),
        (&json!("dsh"), &json!("steady"), &json!([]))
    );
    assert_eq!(wallets(&status["balances"]), cny(94.67002426, false));
    assert_eq!(
        (&status["refused"], &status["failed"]),
        (&json!(false), &Value::Null)
    );
    assert_eq!(status["read_at"], status["balances"][0]["at"]);
    assert!(status["read_next"].is_string(), "{status}");

    // Within the hour it waits, and asks nothing.
    assert_eq!(d.read(&["deepseek"])["outcome"], "waits");
    assert_eq!(d.asks(), 1);
}

#[test]
fn a_refused_grant_is_not_asked_again_until_a_login() {
    let mut d = Dsh::new("refused");
    let home = d.home();
    d.ok(&[
        "account",
        "add",
        "deepseek",
        "--engine",
        "dsh",
        "--machine",
        "mini",
        "--home",
        &home,
    ]);
    d.read(&["deepseek"]);

    // Unreachable: a failed try, the balance kept stale.
    d.set("FAKE_CURL_FAULT", "unreachable");
    let read = d.read(&["deepseek", "--now"]);
    assert_eq!(read["outcome"], "failed", "{read}");
    assert!(
        read["error"]
            .as_str()
            .unwrap()
            .contains("Could not resolve host"),
        "{read}"
    );
    assert_eq!(wallets(&read["balances"]), cny(94.67002426, true));

    // Refused: said, the balance kept stale, and never asked again, `--now` or not.
    d.set("FAKE_CURL_FAULT", "refused");
    let read = d.read(&["deepseek", "--now"]);
    assert_eq!(read["outcome"], "refused", "{read}");
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("invalid token") && error.contains("stablehand account login deepseek"),
        "{error}"
    );
    assert!(!error.contains(GRANT), "{error}");
    assert_eq!(wallets(&read["balances"]), cny(94.67002426, true));
    let asked = d.asks();
    d.set("FAKE_CURL_FAULT", "");
    for args in [&["deepseek"][..], &["deepseek", "--now"]] {
        let again = d.read(args);
        assert_eq!(
            (&again["outcome"], &again["error"]),
            (&json!("refused"), &read["error"])
        );
    }
    assert_eq!(d.asks(), asked, "a refused grant was asked again");
    let status = d.status("deepseek");
    assert_eq!(
        (&status["refused"], &status["failed"]),
        (&json!(true), &read["error"])
    );

    // A login lifts it: the next read asks again.
    d.ok(&["account", "login", "deepseek"]);
    assert_eq!(d.status("deepseek")["refused"], false);
    let read = d.read(&["deepseek", "--now"]);
    assert_eq!(read["outcome"], "read", "{read}");
    assert_eq!(wallets(&read["balances"]), cny(94.67002426, false));
}

#[test]
fn a_portable_login_is_read_with_sesame_s_token_and_a_bound_one_without_its_grant_fails() {
    let d = Dsh::new("portable");
    d.ok(&["account", "add", "deepseek-p", "--engine", "dsh"]);
    assert_eq!(d.read(&["deepseek-p"])["outcome"], "read");
    assert!(
        d.log()
            .contains(&format!("header = \"x-dsh-auth-token: {SESAME_TOKEN}\"")),
        "{}",
        d.log()
    );

    // A bound login whose home holds no grant: failed, naming the fix, asking nothing.
    let nowhere = d.root.join("nowhere").display().to_string();
    d.ok(&[
        "account",
        "add",
        "deepseek",
        "--engine",
        "dsh",
        "--machine",
        "mini",
        "--home",
        &nowhere,
    ]);
    let read = d.read(&["deepseek"]);
    assert_eq!(read["outcome"], "failed", "{read}");
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains(".credentials.yaml") && error.contains("stablehand account login deepseek"),
        "{error}"
    );
    assert_eq!(d.asks(), 1);
}
