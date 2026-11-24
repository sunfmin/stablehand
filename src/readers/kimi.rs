//! kimi's reader: what Kimi Code's own `/usage` asks (2.5.3,
//! `fetchManagedUsage`), `GET https://api.kimi.com/coding/v1/usages` with the
//! account's bearer. Its windows are the answer's `usages`, each under Kimi's
//! own name (`limit_5h`, `limit_7d`, `limit_month_total`, `limit_month_code`,
//! and any other it lists) with a `used_ratio` from 0 to 1 (a number or a
//! numeric string) and an RFC 3339 `reset_time`.
//!
//! A bound login's access token lasts 15 minutes and only Kimi Code refreshes
//! it, in a session (docs/adr/0001: stablehand runs no vendor's OAuth
//! client): one read long after the last session is refused, HTTP 401.
use crate::account::Account;
use crate::books::Window;
use crate::{curl, login, time};
use std::path::PathBuf;

const USAGES: &str = "https://api.kimi.com/coding/v1/usages";

pub fn read(account: &Account) -> Result<Vec<Window>, String> {
    let bearer = bearer(account)?;
    let (status, body) = curl::get(
        USAGES,
        &[
            format!("Authorization: Bearer {bearer}"),
            "Accept: application/json".to_string(),
        ],
    )?;
    windows(status, &body, account)
}

/// The account's home: `--home`, else Kimi Code's own default.
fn home(account: &Account) -> PathBuf {
    match &account.home {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".kimi-code"),
    }
}

/// The account's bearer: a portable login's from its token command
/// ([`login::token`]), a bound one's from the login in its home,
/// `credentials/kimi-code.json` under Kimi Code's `KIMI_CODE_HOME`.
fn bearer(account: &Account) -> Result<String, String> {
    if account.machine.is_none() {
        return login::token(account);
    }
    let file = home(account).join("credentials/kimi-code.json");
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
    creds["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| fix("no access_token in it".into()))
}

/// How long a window of this name is, in minutes, where its name says.
fn minutes(name: &str) -> Option<u64> {
    match name {
        "limit_5h" => Some(5 * 60),
        "limit_7d" => Some(7 * 24 * 60),
        _ => None,
    }
}

/// A ratio from 0 to 1, as Kimi Code reads one (`ratioValue`), in percent
/// to two places.
fn percent(ratio: &serde_json::Value) -> Option<f64> {
    let ratio: f64 = match ratio {
        serde_json::Value::String(s) => s.trim().parse().ok()?,
        other => other.as_f64()?,
    };
    ratio
        .is_finite()
        .then(|| (ratio * 10_000.0).round() / 100.0)
}

/// The windows of the endpoint's answer, `status` and `body`, or why it
/// gave none, with the fix.
fn windows(status: u16, body: &str, account: &Account) -> Result<Vec<Window>, String> {
    let answer: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    if status != 200 {
        let said = answer["error"]["message"]
            .as_str()
            .unwrap_or("no error message");
        let fix = match (status, account.machine.as_deref()) {
            (401, Some(machine)) => format!(
                "the login was refused or its token has expired (Kimi Code refreshes it only in a session): start a kimi session with KIMI_CODE_HOME={} on {machine}, or log in again with {}",
                home(account).display(),
                login::login_line(account)
            ),
            (401, None) => format!(
                "the token was refused or has expired: make a new one and keep it where `{}` reads it",
                account
                    .token_command
                    .as_deref()
                    .unwrap_or("its --token-command")
            ),
            _ => "try again later (`stablehand read --now`)".to_string(),
        };
        return Err(format!("{USAGES} answered HTTP {status}: {said}: {fix}"));
    }
    let found: Vec<Window> = answer["usages"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, w)| {
            Some(Window {
                name: key.clone(),
                group: None,
                used_percent: percent(&w["used_ratio"])?,
                resets_at: w["reset_time"].as_str().and_then(time::parse),
                minutes: minutes(key),
            })
        })
        .collect();
    if found.is_empty() {
        return Err(format!(
            "{USAGES} answered no window: its answer has changed; run `truefit contract --real kimi` in stablehand's repo"
        ));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::Engine;

    fn account() -> Account {
        Account {
            name: "kimi-1".into(),
            engine: Engine::Kimi,
            machine: None,
            sessions: None,
            expires: None,
            home: None,
            off: None,
            token_command: Some("mytokens get kimi".into()),
            identity: None,
        }
    }

    #[test]
    fn the_answer_s_windows_are_its_usages_with_a_ratio() {
        let body = r#"{"limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"limit":"100","remaining":"100"}}],
            "booster_wallet":{"balance":{"amount":"5000000000"}},
            "usages":{"limit_5h":{"used_ratio":0.003,"reset_time":"2026-11-24T10:30:00.711416Z"},
            "limit_7d":{"used_ratio":"0.5","reset_time":"2026-11-29T00:00:00Z"},
            "limit_month_total":{"used_ratio":0.8269,"reset_time":"2026-11-25T00:00:00Z"},
            "limit_month_code":{"used_ratio":0.8269},"limit_none":null,"limit_odd":{"used_ratio":"many"}}}"#;
        let got = windows(200, body, &account()).unwrap();
        let shown: Vec<_> = got
            .iter()
            .map(|w| (w.name.as_str(), w.used_percent, w.resets_at, w.minutes))
            .collect();
        assert_eq!(
            shown,
            [
                ("limit_5h", 0.3, Some(1_795_516_200), Some(300)),
                ("limit_7d", 50.0, Some(1_795_910_400), Some(10_080)),
                ("limit_month_code", 82.69, None, None),
                ("limit_month_total", 82.69, Some(1_795_564_800), None),
            ]
        );
    }

    #[test]
    fn a_refusal_names_its_fix() {
        let bad = r#"{"error":{"message":"The API Key appears to be invalid or may have expired.","type":"invalid_authentication_error"}}"#;
        let fault = windows(401, bad, &account()).unwrap_err();
        assert!(
            fault.contains("make a new one and keep it where `mytokens get kimi` reads it"),
            "{fault}"
        );
        let fault = windows(502, "bad gateway", &account()).unwrap_err();
        assert!(
            fault.contains("HTTP 502: no error message: try again later"),
            "{fault}"
        );
        let fault = windows(200, r#"{"usages":{}}"#, &account()).unwrap_err();
        assert!(fault.contains("answered no window"), "{fault}");
    }
}
