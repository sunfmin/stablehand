//! `statusline`, run through the binary as Claude Code runs it, on stdin
//! samples of what Claude Code gives its status line command
//! (tests/statusline/*.json; their `resets_at` set in 2100, so that the
//! windows have not reset when the test reads them).
mod support;
use serde_json::{Value, json};
use support::Register;

const RATE_LIMITS: &str = include_str!("statusline/rate-limits.json");
const NO_RATE_LIMITS: &str = include_str!("statusline/no-rate-limits.json");

fn window<'a>(account: &'a Value, name: &str) -> &'a Value {
    let windows = account["windows"].as_array().unwrap();
    windows
        .iter()
        .find(|w| w["name"] == name)
        .unwrap_or_else(|| panic!("no {name} in {account}"))
}

fn status(r: &Register, name: &str) -> Value {
    r.ok(&["status", name])["accounts"][0].clone()
}

#[test]
fn the_status_line_books_its_rate_limits_and_prints_one_line() {
    let r = Register::new("statusline-books");
    r.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    let args = ["statusline", "--account", "claude-felix"];

    let (code, line) = r.raw(&args, &[], RATE_LIMITS);
    assert_eq!(
        (code, line.as_str()),
        (0, "claude-felix · 5h 24% · 7d 41%\n")
    );
    let s = status(&r, "claude-felix");
    let five = window(&s, "five_hour");
    assert_eq!(
        (&five["used_percent"], &five["resets_at"], &five["source"]),
        (&json!(23.5), &json!("2100-01-01T00:00:00Z"), &json!("read"))
    );
    let seven = window(&s, "seven_day");
    assert_eq!(
        (&seven["used_percent"], &seven["resets_at"]),
        (&json!(41.2), &json!("2100-01-07T00:00:00Z"))
    );
    assert_eq!(s["windows"].as_array().unwrap().len(), 2, "{s}");

    // A session that has had no answer yet gives no rate_limits: the line
    // is the account's name, and the books keep what they had.
    let (code, line) = r.raw(&args, &[], NO_RATE_LIMITS);
    assert_eq!((code, line.as_str()), (0, "claude-felix\n"));
    assert_eq!(status(&r, "claude-felix"), s);

    // A reading lifts a limit booked without a reset, as `read` does.
    r.ok(&["limit", "claude-felix"]);
    assert_eq!(status(&r, "claude-felix")["pace"], "used_up");
    r.raw(&args, &[], RATE_LIMITS);
    assert_eq!(status(&r, "claude-felix")["limit"], Value::Null);
}

#[test]
fn then_hands_the_same_stdin_to_the_person_s_own_line() {
    let r = Register::new("statusline-then");
    r.ok(&["account", "add", "claude-felix", "--engine", "claude-code"]);
    let stdin = RATE_LIMITS;
    let (code, line) = r.raw(
        &["statusline", "--account", "claude-felix", "--then", "cat"],
        &[],
        stdin,
    );
    // Byte for byte: no newline added, the UTF-8 as it came.
    assert_eq!((code, line.as_str()), (0, stdin));
    assert_eq!(
        window(&status(&r, "claude-felix"), "five_hour")["used_percent"],
        23.5
    );

    // Its exit is the person's line's.
    let (code, line) = r.raw(
        &[
            "statusline",
            "--account",
            "claude-felix",
            "--then",
            "echo mine; exit 3",
        ],
        &[],
        stdin,
    );
    assert_eq!((code, line.as_str()), (3, "mine\n"));
}

#[test]
fn an_account_it_cannot_book_still_gets_a_line() {
    let r = Register::new("statusline-faults");
    r.ok(&["account", "add", "chatgpt", "--engine", "codex"]);
    let stdin = RATE_LIMITS;

    let (code, line) = r.raw(&["statusline", "--account", "nobody"], &[], stdin);
    assert_eq!(code, 0);
    assert!(
        line.starts_with("stablehand: no account `nobody`"),
        "{line}"
    );
    assert_eq!(line.lines().count(), 1, "{line}");

    let (code, line) = r.raw(&["statusline", "--account", "chatgpt"], &[], stdin);
    assert_eq!(code, 0);
    assert!(line.contains("codex"), "{line}");
    assert_eq!(status(&r, "chatgpt")["windows"], json!([]));

    // With --then the person's line is printed whatever the fault.
    let (code, line) = r.raw(
        &["statusline", "--account", "nobody", "--then", "echo mine"],
        &[],
        stdin,
    );
    assert_eq!((code, line.as_str()), (0, "mine\n"));
}
