//! The readers of a ChatGPT account, codex's and pi's: both engines spend
//! one, and both answer its windows `primary` and `secondary`, each its used
//! %, its length and when it resets. codex is asked through its app server
//! (`codex app-server`, JSON-RPC on stdio: `account/rateLimits/read`, which
//! refreshes the login in its `CODEX_HOME` and spends no turn); pi's
//! `openai-codex` login by `GET https://chatgpt.com/backend-api/wham/usage`,
//! the bearer `pi auth print-bearer-token` gives (refreshed where it has
//! expired) and the login's `ChatGPT-Account-Id`. Their logins are bound:
//! each is read on its machine, in its home.
use crate::account::{Account, Engine};
use crate::books::Window;
use crate::{curl, login};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const USAGE: &str = "https://chatgpt.com/backend-api/wham/usage";
/// What the app server is asked: who asks, then the account's windows.
const REQUESTS: &str = concat!(
    r#"{"id":0,"method":"initialize","params":{"clientInfo":{"name":"stablehand","version":"0"}}}"#,
    "\n",
    r#"{"method":"initialized"}"#,
    "\n",
    r#"{"id":1,"method":"account/rateLimits/read","params":{"excludeResetCreditDetails":true}}"#,
    "\n",
);
/// How long the app server may take to answer.
const ANSWER_WITHIN: Duration = Duration::from_secs(20);
/// The windows of a ChatGPT account, by the name both engines give them.
const WINDOWS: [&str; 2] = ["primary", "secondary"];

pub fn codex(account: &Account) -> Result<Vec<Window>, String> {
    let home = home(account)?;
    let mut cmd = Command::new("codex");
    cmd.arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(home) = &home {
        cmd.env("CODEX_HOME", home);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("codex: {e}: install codex, or put it on PATH"))?;
    let stdout = child.stdout.take().expect("piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let answer = BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
            .find(|answer| answer["id"] == 1);
        let _ = tx.send(answer);
    });
    // The app server ends on its input's end before it answers a request in
    // flight: its stdin stays open until the answer comes.
    let mut stdin = child.stdin.take().expect("piped");
    let written = stdin.write_all(REQUESTS.as_bytes());
    let answer = rx.recv_timeout(ANSWER_WITHIN);
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    written.map_err(|e| format!("codex app-server: {e}"))?;
    match answer {
        Ok(Some(answer)) => codex_windows(&answer, &relog(account)),
        Ok(None) => Err(
            "codex app-server ended with no answer to account/rateLimits/read: run `codex app-server` to see why".into(),
        ),
        Err(_) => Err(format!(
            "codex app-server gave no answer in {} s: try again later (`stablehand read --now`)",
            ANSWER_WITHIN.as_secs()
        )),
    }
}

pub fn pi(account: &Account) -> Result<Vec<Window>, String> {
    let home = home(account)?;
    let mut cmd = Command::new("pi");
    cmd.args(["auth", "print-bearer-token", "--provider", "openai-codex"]);
    if let Some(home) = &home {
        cmd.env("PI_CODING_AGENT_DIR", home);
    }
    let relog = relog(account);
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("pi: {e}: install pi, or put it on PATH"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "pi auth print-bearer-token: {}: {relog}",
            said.trim()
        ));
    }
    let bearer = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // pi has written its refreshed login by now: the account's id is in it.
    let file = home
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".pi/agent")
        })
        .join("auth.json");
    let fault = |why: String| format!("{}: {why}: {relog}", file.display());
    let text = std::fs::read_to_string(&file).map_err(|e| fault(e.to_string()))?;
    let auth: Value = serde_json::from_str(&text).map_err(|e| fault(format!("not JSON: {e}")))?;
    let id = auth["openai-codex"]["accountId"]
        .as_str()
        .ok_or_else(|| fault("no openai-codex.accountId in it".into()))?;
    let (status, body) = curl::get(
        USAGE,
        &[
            format!("Authorization: Bearer {bearer}"),
            format!("ChatGPT-Account-Id: {id}"),
        ],
    )?;
    wham_windows(status, &body, &relog)
}

/// The home of the account's bound login on this machine: its `--home`, or
/// none for the engine's own default; refused for a portable account, or
/// one bound to another machine.
fn home(account: &Account) -> Result<Option<PathBuf>, String> {
    let name = &account.name;
    let Some(machine) = &account.machine else {
        return Err(format!(
            "{}'s login is bound: bind account `{name}` with `stablehand account edit {name} --machine <machine> --home <dir>`, then log it in there with `{}`",
            account.engine.name(),
            account.engine.login().bound.join(" ")
        ));
    };
    let here = login::this_machine()?;
    if &here != machine {
        return Err(format!(
            "account `{name}` is bound to {machine} and this machine is {here} ($STABLEHAND_MACHINE, else `hostname -s`): read it on {machine}"
        ));
    }
    Ok(account.home.as_ref().map(PathBuf::from))
}

/// The fix of a login the engine did not take.
fn relog(account: &Account) -> String {
    format!("log it in with {}", login::login_line(account))
}

/// The windows `primary` and `secondary` of `engine`'s `limits`, each keyed
/// its name and `suffix`, where it says how much of it is used: the keys of
/// its used %, its length (in units of `unit` seconds) and when it resets
/// (Unix seconds). None: the answer has changed.
fn found(
    limits: &Value,
    engine: Engine,
    suffix: &str,
    [used, length, resets_at]: [&str; 3],
    unit: u64,
) -> Result<Vec<Window>, String> {
    let found: Vec<Window> = WINDOWS
        .iter()
        .filter_map(|w| {
            let it = &limits[format!("{w}{suffix}")];
            Some(Window {
                name: w.to_string(),
                group: None,
                used_percent: it[used].as_f64()?,
                resets_at: it[resets_at].as_u64(),
                minutes: it[length].as_u64().map(|l| l * unit / 60),
            })
        })
        .collect();
    if found.is_empty() {
        return Err(format!(
            "{engine}'s usage answered no window: its answer has changed; run `truefit contract --real {engine}` in stablehand's repo",
            engine = engine.name()
        ));
    }
    Ok(found)
}

/// The windows of the app server's answer to `account/rateLimits/read`:
/// its `rateLimits`' `primary` and `secondary`, a `null` one not had.
fn codex_windows(answer: &Value, relog: &str) -> Result<Vec<Window>, String> {
    if let Some(said) = answer["error"]["message"].as_str() {
        return Err(format!("codex app-server: {said}: {relog}"));
    }
    found(
        &answer["result"]["rateLimits"],
        Engine::Codex,
        "",
        ["usedPercent", "windowDurationMins", "resetsAt"],
        60,
    )
}

/// The windows of wham/usage's answer, `status` and `body`: its
/// `rate_limit`'s `primary_window` and `secondary_window`.
fn wham_windows(status: u16, body: &str, relog: &str) -> Result<Vec<Window>, String> {
    let answer: Value = serde_json::from_str(body).unwrap_or_default();
    if status != 200 {
        let said = answer["detail"].as_str().unwrap_or("no detail");
        let fix = match status {
            401 | 403 => relog,
            _ => "try again later (`stablehand read --now`)",
        };
        return Err(format!("{USAGE} answered HTTP {status}: {said}: {fix}"));
    }
    found(
        &answer["rate_limit"],
        Engine::Pi,
        "_window",
        ["used_percent", "limit_window_seconds", "reset_at"],
        1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(windows: &[Window]) -> Vec<(&str, f64, Option<u64>, Option<u64>)> {
        windows
            .iter()
            .map(|w| (w.name.as_str(), w.used_percent, w.resets_at, w.minutes))
            .collect()
    }

    #[test]
    fn the_app_server_s_windows_are_its_rate_limits_primary_and_secondary() {
        let answer: Value = serde_json::from_str(
            r#"{"id":1,"result":{"rateLimits":{"limitId":"codex",
            "primary":{"usedPercent":42,"windowDurationMins":300,"resetsAt":1794884400},
            "secondary":null,"credits":{"hasCredits":false}}}}"#,
        )
        .unwrap();
        let got = codex_windows(&answer, "c").unwrap();
        assert_eq!(
            shown(&got),
            [("primary", 42.0, Some(1_794_884_400), Some(300))]
        );
        let refused: Value = serde_json::from_str(r#"{"error":{"code":-32600,"message":"codex account authentication required to read rate limits"},"id":1}"#).unwrap();
        let fault = codex_windows(&refused, "log it in again").unwrap_err();
        assert!(
            fault.contains("authentication required") && fault.contains(": log it in again"),
            "{fault}"
        );
        let fault = codex_windows(&serde_json::json!({"id": 1, "result": {}}), "c").unwrap_err();
        assert!(fault.contains("truefit contract --real codex"), "{fault}");
    }

    #[test]
    fn wham_s_windows_are_its_rate_limit_s_primary_and_secondary_window() {
        let body = r#"{"plan_type":"plus","rate_limit":{"allowed":true,
            "primary_window":{"used_percent":7,"limit_window_seconds":18000,"reset_after_seconds":60,"reset_at":1795516200},
            "secondary_window":{"used_percent":30.5,"limit_window_seconds":604800,"reset_at":1795900000}}}"#;
        let got = wham_windows(200, body, "relog").unwrap();
        assert_eq!(
            shown(&got),
            [
                ("primary", 7.0, Some(1_795_516_200), Some(300)),
                ("secondary", 30.5, Some(1_795_900_000), Some(10_080)),
            ]
        );
        let bad = r#"{"detail":"Could not parse your authentication token. Please try signing in again."}"#;
        let fault = wham_windows(401, bad, "log it in again").unwrap_err();
        assert!(
            fault.contains("HTTP 401: Could not parse") && fault.ends_with("log it in again"),
            "{fault}"
        );
        let fault = wham_windows(502, "<html>", "r").unwrap_err();
        assert!(
            fault.contains("HTTP 502: no detail: try again later"),
            "{fault}"
        );
        let fault =
            wham_windows(200, r#"{"rate_limit":{"primary_window":null}}"#, "r").unwrap_err();
        assert!(fault.contains("truefit contract --real pi"), "{fault}");
    }
}
