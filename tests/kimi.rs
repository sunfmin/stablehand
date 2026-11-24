//! kimi's reader through the binary: `read` and `status` against the stand-in
//! of curl answering api.kimi.com/coding/v1/usages (tests/contract/kimi-fake-curl)
//! and sesame's.
mod support;
use serde_json::{Value, json};
use support::Register;

/// The bound login's access token, and the portable one sesame keeps: stand-ins.
const LOGIN_TOKEN: &str = "kimi-standin-login";
const SESAME_TOKEN: &str = "sk-kimi-standin-portable";

struct Kimi {
    r: Register,
    fault: std::cell::RefCell<&'static str>,
}

impl Kimi {
    fn new(name: &str) -> Kimi {
        let r = Register::new(name);
        let fake =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contract/kimi-fake-curl");
        std::os::unix::fs::symlink(fake, r.path("bin/curl")).unwrap();
        std::fs::create_dir_all(r.path("kimi/credentials")).unwrap();
        std::fs::write(
            r.path("kimi/credentials/kimi-code.json"),
            json!({ "access_token": LOGIN_TOKEN, "refresh_token": "kept", "expires_in": 900 })
                .to_string(),
        )
        .unwrap();
        Kimi {
            r,
            fault: "".into(),
        }
    }

    fn ok(&self, args: &[&str]) -> Value {
        let log = self.r.path("curl.log");
        let out = self.r.run_with(
            args,
            &[
                ("FAKE_CURL_LOG", &log),
                ("FAKE_CURL_FAULT", *self.fault.borrow()),
            ],
            "",
        );
        assert_eq!(out.code, 0, "{args:?}: {}", out.json);
        out.json
    }

    fn read(&self, args: &[&str]) -> Value {
        self.ok(&[&["read"], args].concat())["reads"][0].clone()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.r.path("curl.log")).unwrap_or_default()
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
fn read_and_status_book_a_bound_kimi_login_s_usages() {
    let k = Kimi::new("kimi-bound");
    let home = k.r.path("kimi");
    k.ok(&[
        "account",
        "add",
        "kimi-mini",
        "--engine",
        "kimi",
        "--machine",
        "mini",
        "--home",
        &home,
    ]);
    let read = k.read(&["kimi-mini"]);
    assert_eq!(read["outcome"], "read", "{read}");
    let five = window(&read["windows"], "limit_5h");
    assert_eq!(
        (&five["used_percent"], &five["resets_at"], &five["source"]),
        (&json!(0.3), &json!("2026-11-24T10:34:05Z"), &json!("read"))
    );
    assert_eq!(
        window(&read["windows"], "limit_month_total")["used_percent"],
        82.69
    );
    assert_eq!(read["windows"].as_array().unwrap().len(), 3, "{read}");

    // The bearer goes to curl on its stdin, never in its arguments.
    let log = k.log();
    let (args, config) = log.split_once('\n').unwrap();
    assert!(
        args.ends_with("https://api.kimi.com/coding/v1/usages"),
        "{args}"
    );
    assert!(!args.contains(LOGIN_TOKEN), "{args}");
    assert!(
        config.contains(&format!("header = \"Authorization: Bearer {LOGIN_TOKEN}\"")),
        "{config}"
    );

    let status = k.ok(&["status", "kimi-mini"])["accounts"][0].clone();
    assert_eq!(
        (&status["engine"], &status["failed"]),
        (&json!("kimi"), &Value::Null)
    );
    assert_eq!(
        window(&status["windows"], "limit_month_code")["stale"],
        false
    );
}

#[test]
fn a_refused_kimi_token_keeps_the_last_windows_stale_and_waits_the_hour() {
    let k = Kimi::new("kimi-refused");
    let home = k.r.path("kimi");
    k.ok(&[
        "account",
        "add",
        "kimi-mini",
        "--engine",
        "kimi",
        "--machine",
        "mini",
        "--home",
        &home,
    ]);
    k.read(&["kimi-mini"]);
    *k.fault.borrow_mut() = "unauthorized";
    let read = k.read(&["kimi-mini", "--now"]);
    assert_eq!(read["outcome"], "failed", "{read}");
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("HTTP 401: The API Key appears to be invalid")
            && error.contains(&format!("KIMI_CODE_HOME={home} on mini")),
        "{error}"
    );
    assert!(!error.contains(LOGIN_TOKEN), "{error}");
    let five = window(&read["windows"], "limit_5h");
    assert_eq!(
        (&five["used_percent"], &five["stale"]),
        (&json!(0.3), &json!(true))
    );
    let status = k.ok(&["status", "kimi-mini"])["accounts"][0].clone();
    assert_eq!(status["failed"], read["error"]);

    // The refused token is not asked again before the hour.
    *k.fault.borrow_mut() = "";
    assert_eq!(k.read(&["kimi-mini"])["outcome"], "waits");
    assert_eq!(k.log().matches("/usages").count(), 2);

    // No login file in its home is a failed try naming its fix.
    std::fs::remove_file(k.r.path("kimi/credentials/kimi-code.json")).unwrap();
    let error = k.read(&["kimi-mini", "--now"])["error"].clone();
    assert!(
        error
            .as_str()
            .unwrap()
            .contains("credentials/kimi-code.json")
            && error
                .as_str()
                .unwrap()
                .contains("stablehand account login kimi-mini"),
        "{error}"
    );
}

#[test]
fn a_portable_kimi_login_is_read_with_sesame_s_token() {
    let k = Kimi::new("kimi-portable");
    k.ok(&["account", "add", "kimi-1", "--engine", "kimi"]);
    let read = k.read(&["kimi-1"]);
    let error = read["error"].as_str().unwrap();
    assert!(
        error.contains("sesame keeps no token")
            && error.contains("stablehand account login kimi-1 --token"),
        "{error}"
    );
    let login = k.r.run_with(
        &["account", "login", "kimi-1", "--token"],
        &[],
        SESAME_TOKEN,
    );
    assert_eq!(login.code, 0, "{}", login.json);
    let read = k.read(&["kimi-1", "--now"]);
    assert_eq!(read["outcome"], "read", "{read}");
    assert!(k.log().contains(&format!("Bearer {SESAME_TOKEN}")));
}
