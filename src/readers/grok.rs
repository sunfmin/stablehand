//! grok's reader: `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits`
//! with the bearer of the account's grok login, `<home>/auth.json`'s
//! `https://auth.x.ai::<id>` entry (`GROK_HOME`; `~/.grok` where it names
//! none), and `x-grok-client-version` as grok sends it. Its windows are the
//! credit period's (`weekly`, `USAGE_PERIOD_TYPE_WEEKLY`): the account's
//! `creditUsagePercent`, and one per product of `productUsage`, the product
//! its group. The token lasts a few hours: one expired is refreshed by grok
//! itself (`grok models`, no model turn), never by stablehand. `grok usage`
//! cannot show a plan's usage outside a session: it is not used.
use crate::account::Account;
use crate::books::Window;
use crate::{curl, time};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BILLING: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const ISSUER: &str = "https://auth.x.ai::";

pub fn read(account: &Account) -> Result<Vec<Window>, String> {
    let name = &account.name;
    let Some(machine) = &account.machine else {
        return Err(format!(
            "a grok login lasts hours, and grok refreshes it in its home: bind `{name}` with `stablehand account edit {name} --machine <machine> --home <its GROK_HOME>`"
        ));
    };
    let home = match &account.home {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".grok"),
    };
    let login_again =
        format!("log it in again on {machine} with `stablehand account login {name}`");
    let now = crate::now()?;
    let load = || {
        let login = Login::of(&home).map_err(|why| format!("{why}: {login_again}"))?;
        Ok::<_, String>(login.usable(now).then_some(login))
    };
    let login = match load()? {
        Some(login) => login,
        None => {
            refresh(account, &home);
            load()?.ok_or_else(|| {
                format!(
                    "the grok login in {} has expired and `grok models` did not refresh it: {login_again}",
                    home.display()
                )
            })?
        }
    };
    let mut headers = vec![format!("Authorization: Bearer {}", login.key)];
    headers.extend(client_version(&home).map(|v| format!("x-grok-client-version: {v}")));
    let (status, body) = curl::get(BILLING, &headers)?;
    windows(status, &body, &login_again)
}

/// The bearer of the grok login in `home`, and when it expires.
struct Login {
    key: String,
    expires_at: u64,
}

impl Login {
    /// Whether it may be sent at `now`: one expiring within the minute is
    /// as good as expired.
    fn usable(&self, now: u64) -> bool {
        self.expires_at > now + 60
    }

    fn of(home: &Path) -> Result<Login, String> {
        let file = home.join("auth.json");
        let at = |why: String| format!("{}: {why}", file.display());
        let text = std::fs::read_to_string(&file).map_err(|e| at(e.to_string()))?;
        let auth: Value = serde_json::from_str(&text).map_err(|e| at(format!("not JSON: {e}")))?;
        let entry = auth
            .as_object()
            .into_iter()
            .flatten()
            .find_map(|(k, v)| k.starts_with(ISSUER).then_some(v))
            .ok_or_else(|| at(format!("no {ISSUER}<id> login in it")))?;
        let key = entry["key"].as_str().map(str::to_string);
        let expires_at = entry["expires_at"].as_str().and_then(time::parse);
        match (key, expires_at) {
            (Some(key), Some(expires_at)) => Ok(Login { key, expires_at }),
            _ => Err(at("its login has no key or expires_at".into())),
        }
    }
}

/// Has grok refresh the login in `home`: `grok models` reads the login
/// and refreshes an expired one with its refresh token, as any start of
/// grok does. What came of it is what `auth.json` says after.
fn refresh(account: &Account, home: &Path) {
    let mut grok = Command::new("grok");
    grok.arg("models")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if account.home.is_some() {
        grok.env("GROK_HOME", home);
    }
    let _ = grok.status();
}

/// The grok version its home last ran, as grok names itself to the proxy.
fn client_version(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join("version.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v["version"].as_str().map(str::to_string)
}

/// The windows of the billing answer, `status` and `body`, or why it gave
/// none, with the fix.
fn windows(status: u16, body: &str, login_again: &str) -> Result<Vec<Window>, String> {
    let answer: Value = serde_json::from_str(body).unwrap_or_default();
    if status != 200 {
        let said = answer["error"].as_str().unwrap_or("no error message");
        let fix = match status {
            401 | 403 => format!("the login was refused: {login_again}"),
            _ => "try again later (`stablehand read --now`)".to_string(),
        };
        return Err(format!("{BILLING} answered HTTP {status}: {said}: {fix}"));
    }
    let config = &answer["config"];
    let period = &config["currentPeriod"];
    let start = period["start"].as_str().and_then(time::parse);
    let end = period["end"].as_str().and_then(time::parse);
    let name = period["type"]
        .as_str()
        .map(|t| t.trim_start_matches("USAGE_PERIOD_TYPE_").to_lowercase());
    let (Some(name), Some(used)) = (name, config["creditUsagePercent"].as_f64()) else {
        return Err(format!(
            "{BILLING} answered no credit period: its answer has changed; run `truefit contract --real grok` in stablehand's repo"
        ));
    };
    let window = |group: Option<String>, used_percent| Window {
        name: name.clone(),
        used_percent,
        resets_at: end,
        // A product's is its share of the account's credits, not a pool of
        // its own: given no length, it is never paced on its own.
        minutes: start
            .zip(end)
            .filter(|_| group.is_none())
            .map(|(s, e)| e.saturating_sub(s) / 60),
        group,
    };
    let products = config["productUsage"].as_array().into_iter().flatten();
    Ok(std::iter::once(window(None, used))
        .chain(products.filter_map(|p| {
            Some(window(
                Some(p["product"].as_str()?.to_string()),
                p["usagePercent"].as_f64()?,
            ))
        }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_period_is_a_window_and_each_product_one_in_its_group() {
        let body = r#"{"config":{"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY",
            "start":"2026-11-17T11:06:25.960+00:00","end":"2026-11-24T11:06:25.960+00:00"},
            "creditUsagePercent":92.0,"productUsage":[{"product":"Api","usagePercent":88.0},
            {"product":"GrokBuild","usagePercent":4},{"usagePercent":1}],"prepaidBalance":{"val":1765}}}"#;
        let got = windows(200, body, "x").unwrap();
        let shown: Vec<_> = got
            .iter()
            .map(|w| {
                let g = w.group.as_deref();
                (w.name.as_str(), g, w.used_percent, w.resets_at, w.minutes)
            })
            .collect();
        let resets = Some(1_795_518_385);
        assert_eq!(
            shown,
            [
                ("weekly", None, 92.0, resets, Some(10_080)),
                ("weekly", Some("Api"), 88.0, resets, None),
                ("weekly", Some("GrokBuild"), 4.0, resets, None),
            ]
        );
    }

    #[test]
    fn a_refusal_names_its_fix() {
        let bad = r#"{"error":"Invalid or expired credentials (auth_kind=bearer)"}"#;
        let fault = windows(401, bad, "log it in again").unwrap_err();
        assert!(
            fault.contains("HTTP 401: Invalid or expired credentials")
                && fault.contains("refused: log it in again"),
            "{fault}"
        );
        let fault = windows(502, "bad gateway", "x").unwrap_err();
        assert!(
            fault.contains("HTTP 502: no error message: try again later"),
            "{fault}"
        );
        let fault = windows(200, r#"{"config":{}}"#, "x").unwrap_err();
        assert!(fault.contains("answered no credit period"), "{fault}");
    }

    #[test]
    fn the_login_is_the_x_ai_entry_of_auth_json() {
        let home = std::env::temp_dir().join(format!("stablehand-grok-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let write = |text: &str| std::fs::write(home.join("auth.json"), text).unwrap();
        write(
            r#"{"other::1":{"key":"no"},"https://auth.x.ai::b1":{"key":"stand-in","expires_at":"2026-11-24T12:19:51.419388Z"}}"#,
        );
        let login = Login::of(&home).unwrap();
        assert_eq!(
            (login.key.as_str(), login.expires_at),
            ("stand-in", 1_795_522_791)
        );
        assert!(login.usable(login.expires_at - 61) && !login.usable(login.expires_at - 60));
        write(r#"{"https://auth.x.ai::b1":{"key":"stand-in"}}"#);
        let fault = Login::of(&home).err().unwrap();
        assert!(fault.contains("no key or expires_at"), "{fault}");
        write("{}");
        let fault = Login::of(&home).err().unwrap();
        assert!(
            fault.contains("no https://auth.x.ai::<id> login"),
            "{fault}"
        );
        std::fs::write(home.join("version.json"), r#"{"version":"1.0.92"}"#).unwrap();
        assert_eq!(client_version(&home).as_deref(), Some("1.0.92"));
        std::fs::remove_dir_all(&home).unwrap();
        assert_eq!(client_version(&home), None);
    }
}
