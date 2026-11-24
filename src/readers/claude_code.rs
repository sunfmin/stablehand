//! claude-code's reader. It asks `GET https://api.anthropic.com/api/oauth/usage`
//! with the account's OAuth bearer and `anthropic-beta: oauth-2025-04-20`:
//! its windows are the answer's objects with a `utilization` and a
//! `resets_at`, `five_hour`, `seven_day`, and any other it lists. A `claude
//! setup-token` token is refused it (HTTP 403, `oauth_scope_insufficient`):
//! such a token is asked the [`QUESTION`] instead, one turn on the smallest
//! model, whose stream's `rate_limit_event` says the windows; and, once read
//! so ([`ReadBy::Question`]), it is asked the question again, not its usage.
use crate::account::Account;
use crate::books::{ReadBy, Window};
use crate::bounded;
use crate::{curl, login, time};
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const USAGE: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA: &str = "anthropic-beta: oauth-2025-04-20";

/// The question a token refused its usage is asked: `claude` with these
/// arguments, one turn on its smallest model, with no tools, settings,
/// servers or kept session (afkconvoy's `WINDOWS_QUESTION`, recorded with
/// Claude Code 2.1.326: about 3 s, under $0.0002 a time). It spends the
/// account's quota.
pub const QUESTION: &[&str] = &[
    "-p",
    "--model",
    "haiku",
    "--output-format",
    "stream-json",
    "--verbose",
    "--tools",
    "",
    "--strict-mcp-config",
    "--no-session-persistence",
    "--setting-sources",
    "",
    "--system-prompt",
    "Answer with one word.",
    "quota",
];

/// How long the question may take: it retries a refused token for a while
/// before it says so.
const QUESTION_LIMIT: Duration = Duration::from_secs(90);

/// `account`'s windows: asked of its usage, unless its latest reading was by
/// the question (`read_by`) or its usage refuses its token; then by the
/// question.
pub fn read(account: &Account, read_by: Option<ReadBy>) -> Result<Vec<Window>, String> {
    let bearer = bearer(account)?;
    if read_by != Some(ReadBy::Question) {
        let (status, body) = curl::get(
            USAGE,
            &[format!("Authorization: Bearer {bearer}"), BETA.to_string()],
        )?;
        let answer: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        // A `claude setup-token` token is refused its scope.
        let scope = answer["error"]["details"]["error_code"] == "oauth_scope_insufficient";
        if !(status == 403 && scope) {
            return windows(status, &answer);
        }
    }
    question(account, &bearer)
}

/// The windows the [`QUESTION`] answers, asked with `bearer` in
/// `CLAUDE_CODE_OAUTH_TOKEN` (on no command line), in a config dir of its
/// own, new and empty, which it is also run in and which is removed after:
/// nothing of a person's `~/.claude`, nor of the directory stablehand runs
/// in, is read.
fn question(account: &Account, bearer: &str) -> Result<Vec<Window>, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "stablehand-question-{}-{nanos}",
        std::process::id()
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut claude = Command::new("claude");
    claude
        .args(QUESTION)
        .current_dir(&dir)
        .env("CLAUDE_CONFIG_DIR", &dir)
        .env("CLAUDE_CODE_OAUTH_TOKEN", bearer)
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let ran = bounded::answer(
        claude,
        QUESTION_LIMIT,
        "`claude -p … quota`",
        "install Claude Code (`curl -fsSL https://claude.ai/install.sh | bash`), or put it on PATH",
    );
    let _ = std::fs::remove_dir_all(&dir);
    let (code, stdout) = ran?;
    stream(code, &stdout, account)
}

/// The windows of the question's stream, its exit `code` and `stdout`: the
/// last `rate_limit_event`'s `rate_limit_info.unifiedWindows`, each a
/// `utilization` from 0 to 1 and a `resetsAt` (seconds since the Unix
/// epoch); or why it gave none, with the fix.
fn stream(code: Option<i32>, stdout: &str, account: &Account) -> Result<Vec<Window>, String> {
    // The latest record of `kind`, the stream read from its end.
    let last = |kind: &str| {
        stdout
            .lines()
            .rev()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|r| r["type"] == kind)
    };
    let event = last("rate_limit_event").unwrap_or_default();
    let found: Vec<Window> = event["rate_limit_info"]["unifiedWindows"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, w)| {
            Some(Window {
                name: name.clone(),
                group: None,
                models: Vec::new(),
                // In percent, to a hundredth: 0.07 is 7, not 7.000000000000001.
                used_percent: (w["utilization"].as_f64()? * 10_000.0).round() / 100.0,
                resets_at: w["resetsAt"].as_f64().map(|at| at as u64),
                minutes: minutes(name),
                read_by: Some(ReadBy::Question),
            })
        })
        .collect();
    if !found.is_empty() {
        return Ok(found);
    }
    let exit = code.map_or("was killed".into(), |c| format!("exited {c}"));
    let result = last("result");
    let said = result
        .as_ref()
        .and_then(|r| r["result"].as_str())
        .unwrap_or("no result said");
    let fix = if result
        .as_ref()
        .is_some_and(|r| r["api_error_status"] == 401)
    {
        format!(
            "the token was refused or has expired: {}",
            login::login_line(account)
        )
    } else if code == Some(0) {
        "its stream has changed: run `truefit contract --real claude-code` in stablehand's repo"
            .to_string()
    } else {
        "try again later (`stablehand read --now`)".to_string()
    };
    Err(format!("`claude -p … quota` {exit}: {said}: {fix}"))
}

/// The account's access token: a portable login's from its token command
/// ([`login::token`]), a bound one's from the login in its
/// home, Claude Code's `CLAUDE_CONFIG_DIR` (`~/.claude` where it names none).
fn bearer(account: &Account) -> Result<String, String> {
    if account.machine.is_none() {
        return login::token(account);
    }
    let home = match &account.home {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude"),
    };
    let file = home.join(".credentials.json");
    let fix = |why: String| {
        format!(
            "{}: {why}: log it in with {}",
            file.display(),
            login::login_line(account)
        )
    };
    let text = std::fs::read_to_string(&file).map_err(|e| fix(e.to_string()))?;
    let creds: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| fix(format!("not JSON: {e}")))?;
    creds["claudeAiOauth"]["accessToken"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| fix("no claudeAiOauth.accessToken in it".into()))
}

/// The windows of what Claude Code gives its status line command on stdin:
/// each of its `rate_limits` (`five_hour`, `seven_day`) with a
/// `used_percentage`, resetting at its `resets_at` (seconds since the Unix
/// epoch). None where it gives none, as before a session's first answer.
pub fn statusline(stdin: &[u8]) -> Vec<Window> {
    let given: serde_json::Value = serde_json::from_slice(stdin).unwrap_or_default();
    given["rate_limits"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, w)| {
            Some(Window {
                name: name.clone(),
                group: None,
                models: Vec::new(),
                used_percent: w["used_percentage"].as_f64()?,
                resets_at: w["resets_at"].as_u64(),
                minutes: minutes(name),
                read_by: None,
            })
        })
        .collect()
}

/// How long a window of this name is, in minutes, where its name says.
fn minutes(name: &str) -> Option<u64> {
    if name.starts_with("five_hour") {
        Some(5 * 60)
    } else if name.starts_with("seven_day") {
        Some(7 * 24 * 60)
    } else {
        None
    }
}

/// The windows of the endpoint's answer, `status` and its JSON `answer`, or why it
/// gave none, with the fix.
fn windows(status: u16, answer: &serde_json::Value) -> Result<Vec<Window>, String> {
    if status != 200 {
        let error = &answer["error"];
        let said = error["message"].as_str().unwrap_or("no error message");
        let fix = match status {
            401 => "the login was refused or has expired: log in again",
            _ => "try again later (`stablehand read --now`)",
        };
        return Err(format!("{USAGE} answered HTTP {status}: {said}: {fix}"));
    }
    let found: Vec<Window> = answer
        .as_object()
        .into_iter()
        .flatten()
        // A window says when it resets, if only `null`: `extra_usage`, the
        // month's credits, has a `utilization` and no `resets_at`.
        .filter(|(_, w)| w.get("resets_at").is_some())
        .filter_map(|(key, w)| {
            Some(Window {
                name: key.clone(),
                group: None,
                models: Vec::new(),
                used_percent: w["utilization"].as_f64()?,
                resets_at: w["resets_at"].as_str().and_then(time::parse),
                minutes: minutes(key),
                read_by: Some(ReadBy::Usage),
            })
        })
        .collect();
    if found.is_empty() {
        return Err(format!(
            "{USAGE} answered no window: its answer has changed; run `truefit contract --real claude-code` in stablehand's repo"
        ));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_s_windows_are_its_objects_with_a_utilization_and_a_reset() {
        let body = r#"{"five_hour":{"utilization":22.0,"resets_at":"2026-11-24T10:30:00.029137+00:00","limit_dollars":null},
            "seven_day":{"utilization":52,"resets_at":null},"seven_day_opus":null,
            "extra_usage":{"is_enabled":true,"utilization":40.0,"monthly_limit":5000},"limits":[{"percent":22}]}"#;
        let got = windows(200, &serde_json::from_str(body).unwrap()).unwrap();
        let shown: Vec<_> = got
            .iter()
            .map(|w| (w.name.as_str(), w.used_percent, w.resets_at, w.minutes))
            .collect();
        assert_eq!(
            shown,
            [
                ("five_hour", 22.0, Some(1_795_516_200), Some(300)),
                ("seven_day", 52.0, None, Some(10_080)),
            ]
        );
        assert_eq!(minutes("tangelo"), None);
    }

    #[test]
    fn a_refusal_names_its_fix() {
        let json = |s: &str| serde_json::from_str(s).unwrap_or_default();
        let bad = r#"{"type":"error","error":{"type":"authentication_error","message":"Invalid bearer token"}}"#;
        let fault = windows(401, &json(bad)).unwrap_err();
        assert!(
            fault.contains("Invalid bearer token") && fault.contains("log in again"),
            "{fault}"
        );
        let fault = windows(529, &json("overloaded")).unwrap_err();
        assert!(
            fault.contains("HTTP 529: no error message: try again later"),
            "{fault}"
        );
        let fault = windows(200, &json(r#"{"spend":{}}"#)).unwrap_err();
        assert!(fault.contains("answered no window"), "{fault}");
    }

    fn portable() -> Account {
        Account {
            name: "claude-jp".into(),
            engine: crate::account::Engine::ClaudeCode,
            machine: None,
            expires: None,
            home: None,
            off: None,
            token_command: None,
            identity: None,
        }
    }

    #[test]
    fn the_question_s_windows_are_its_last_rate_limit_event_s_unified_windows() {
        let recorded = include_str!("../../tests/contract/recorded/claude-code-question.jsonl");
        let got = stream(Some(0), recorded, &portable()).unwrap();
        let shown: Vec<_> = got
            .iter()
            .map(|w| {
                (
                    w.name.as_str(),
                    w.used_percent,
                    w.resets_at,
                    w.minutes,
                    w.read_by,
                )
            })
            .collect();
        let q = Some(ReadBy::Question);
        assert_eq!(
            shown,
            [
                ("five_hour", 7.0, Some(1_794_884_400), Some(300), q),
                ("seven_day", 45.0, Some(1_795_345_200), Some(10_080), q),
            ]
        );
    }

    #[test]
    fn a_question_with_no_windows_says_why_with_its_fix() {
        let refused = r#"{"type":"result","is_error":true,"api_error_status":401,"result":"Failed to authenticate. API Error: 401 OAuth access token is invalid."}"#;
        let fault = stream(Some(1), refused, &portable()).unwrap_err();
        assert!(
            fault.contains("exited 1: Failed to authenticate")
                && fault.contains("claude setup-token")
                && fault.contains("stablehand account edit claude-jp"),
            "{fault}"
        );
        let fault = stream(
            Some(0),
            r#"{"type":"result","result":"Unknown"}"#,
            &portable(),
        )
        .unwrap_err();
        assert!(fault.contains("its stream has changed"), "{fault}");
        let fault = stream(None, "", &portable()).unwrap_err();
        assert!(
            fault.contains("was killed: no result said: try again later"),
            "{fault}"
        );
    }
}
