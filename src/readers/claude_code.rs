//! claude-code's reader: `GET https://api.anthropic.com/api/oauth/usage`
//! with the account's OAuth bearer and `anthropic-beta: oauth-2025-04-20`.
//! Its windows are the answer's objects with a `utilization` and a
//! `resets_at`: `five_hour`, `seven_day`, and any other it lists. A
//! `claude setup-token` token is refused it (HTTP 403, `oauth_scope_insufficient`): such an account is
//! known by `stablehand limit` alone.
use crate::account::Account;
use crate::books::Window;
use crate::{curl, login, time};
use std::path::PathBuf;

const USAGE: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA: &str = "anthropic-beta: oauth-2025-04-20";

pub fn read(account: &Account) -> Result<Vec<Window>, String> {
    let bearer = bearer(account)?;
    let (status, body) = curl::get(
        USAGE,
        &[format!("Authorization: Bearer {bearer}"), BETA.to_string()],
    )?;
    windows(status, &body, &account.name)
}

/// The account's access token: a portable login's from sesame (as
/// `stablehand account login` keeps it), a bound one's from the login in its
/// home, Claude Code's `CLAUDE_CONFIG_DIR` (`~/.claude` where it names none).
fn bearer(account: &Account) -> Result<String, String> {
    let name = &account.name;
    if account.machine.is_none() {
        return login::token(account)?.ok_or_else(|| {
            format!("sesame keeps no token for `{name}`: log it in with `stablehand account login {name}`")
        });
    }
    let home = match &account.home {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude"),
    };
    let file = home.join(".credentials.json");
    let fix = |why: String| {
        format!(
            "{}: {why}: log it in on {} with `stablehand account login {name}`",
            file.display(),
            account.machine.as_deref().unwrap_or_default()
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
    let Some(limits) = given["rate_limits"].as_object() else {
        return Vec::new();
    };
    limits
        .iter()
        .filter_map(|(name, w)| {
            Some(Window {
                name: name.clone(),
                group: None,
                used_percent: w["used_percentage"].as_f64()?,
                resets_at: w["resets_at"].as_u64(),
                minutes: minutes(name),
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

/// The windows of the endpoint's answer, `status` and `body`, or why it
/// gave none, with the fix.
fn windows(status: u16, body: &str, name: &str) -> Result<Vec<Window>, String> {
    let answer: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    if status != 200 {
        let error = &answer["error"];
        let said = error["message"].as_str().unwrap_or("no error message");
        let fix = match (status, error["details"]["error_code"].as_str()) {
            (403, Some("oauth_scope_insufficient")) => format!(
                "a `claude setup-token` token is given no usage: report a limit its sessions meet with `stablehand limit {name}`"
            ),
            (401, _) => "the login was refused or has expired: log in again".to_string(),
            _ => "try again later (`stablehand read --now`)".to_string(),
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
                used_percent: w["utilization"].as_f64()?,
                resets_at: w["resets_at"].as_str().and_then(time::parse),
                minutes: minutes(key),
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
        let got = windows(200, body, "c").unwrap();
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
        let scope = r#"{"type":"error","error":{"type":"permission_error","message":"OAuth token does not meet scope requirement user:profile","details":{"error_code":"oauth_scope_insufficient"}}}"#;
        let fault = windows(403, scope, "claude-felix").unwrap_err();
        assert!(
            fault.contains("HTTP 403") && fault.contains("stablehand limit claude-felix"),
            "{fault}"
        );
        let bad = r#"{"type":"error","error":{"type":"authentication_error","message":"Invalid bearer token"}}"#;
        let fault = windows(401, bad, "c").unwrap_err();
        assert!(
            fault.contains("Invalid bearer token") && fault.contains("log in again"),
            "{fault}"
        );
        let fault = windows(529, "overloaded", "c").unwrap_err();
        assert!(
            fault.contains("HTTP 529: no error message: try again later"),
            "{fault}"
        );
        let fault = windows(200, r#"{"spend":{}}"#, "c").unwrap_err();
        assert!(fault.contains("answered no window"), "{fault}");
    }
}
