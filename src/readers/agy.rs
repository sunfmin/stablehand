//! agy's reader: `agy -p /usage --output-format json`, run in the account's
//! home (`HOME`), answers the quota with no model turn and no conversation
//! made. Its windows are `command.data.groups[].buckets[]`, a group per
//! model group: `id` (`gemini-5h`), `window` (`5h`, `weekly`),
//! `remaining_fraction` (used is 1 − it) and `reset_time`, UTC, 1970 for a
//! window not yet used. An API-key login exits 2: it has no account quota.
use crate::account::Account;
use crate::books::Window;
use crate::bounded;
use crate::{login, time};
use std::process::{Command, Stdio};
use std::time::Duration;

/// How long agy may take to answer. A home it is not signed in to waits 60
/// seconds for a browser sign-in, then says so: this is past that.
const TIME_LIMIT: Duration = Duration::from_secs(90);

pub fn read(account: &Account) -> Result<Vec<Window>, String> {
    let name = &account.name;
    let Some(machine) = &account.machine else {
        return Err(format!(
            "agy keeps its login in its home, not a token: bind `{name}`: `stablehand account edit {name} --machine <machine> --home <dir>`"
        ));
    };
    let here = login::this_machine()?;
    if *machine != here {
        return Err(format!(
            "`{name}` is bound to {machine} and this machine is {here}: read it on {machine}"
        ));
    }
    let home = match &account.home {
        Some(home) => home.clone(),
        None => std::env::var("HOME").unwrap_or_default(),
    };
    let mut agy = Command::new("agy");
    agy.args(["-p", "/usage", "--output-format", "json"])
        .env("HOME", &home)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let (code, stdout) = bounded::answer(agy, TIME_LIMIT, "agy", "install agy, or put it on PATH")?;
    windows(code, &stdout, &login::login_line(account))
}

/// The windows of agy's answer, its exit `code` and `stdout`, or why it
/// gave none, with the fix.
fn windows(code: Option<i32>, stdout: &str, relog: &str) -> Result<Vec<Window>, String> {
    let answer: serde_json::Value = serde_json::from_str(stdout).unwrap_or_default();
    if code != Some(0) || answer["status"] != "SUCCESS" {
        let said = answer["error"].as_str().unwrap_or("no error said");
        let fix = match code {
            Some(2) => {
                "an API-key login has no account quota: log it in with its Google account".into()
            }
            _ if said.contains("authentication") => {
                format!("its home is not signed in: log it in with {relog}")
            }
            _ => "try again later (`stablehand read --now`)".to_string(),
        };
        let exit = code.map_or("was killed".into(), |c| format!("exited {c}"));
        return Err(format!("`agy -p /usage` {exit}: {said}: {fix}"));
    }
    let groups = answer["command"]["data"]["groups"].as_array();
    let found: Vec<Window> = groups
        .into_iter()
        .flatten()
        .flat_map(|g| {
            let group = g["name"].as_str().map(str::to_string);
            let buckets = g["buckets"].as_array().into_iter().flatten();
            buckets.filter_map(move |b| {
                let left = b["remaining_fraction"].as_f64()?;
                Some(Window {
                    name: b["id"].as_str()?.to_string(),
                    group: group.clone(),
                    read_by: None,
                    used_percent: (1.0 - left) * 100.0,
                    // 1970 (0) is a window not yet used: it has no reset.
                    resets_at: b["reset_time"]
                        .as_str()
                        .and_then(time::parse)
                        .filter(|at| *at > 0),
                    minutes: match b["window"].as_str() {
                        Some("5h") => Some(5 * 60),
                        Some("weekly") => Some(7 * 24 * 60),
                        _ => None,
                    },
                })
            })
        })
        .collect();
    if found.is_empty() {
        return Err(
            "`agy -p /usage` answered no window: its answer has changed; run `truefit contract --real agy` in stablehand's repo".into(),
        );
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_group_s_buckets_are_its_windows_and_1970_is_unused() {
        let body = r#"{"status":"SUCCESS","num_turns":0,"command":{"name":"usage","data":{"groups":[
            {"name":"Gemini Models","buckets":[
              {"id":"gemini-weekly","window":"weekly","remaining_fraction":0.75,"reset_time":"2026-12-01T01:58:11Z"},
              {"id":"gemini-5h","window":"5h","remaining_fraction":0,"reset_time":"2026-11-24T11:58:11Z"}]},
            {"name":"Claude and GPT models","buckets":[
              {"id":"3p-5h","window":"5h","remaining_fraction":1,"reset_time":"1970-01-01T00:00:00Z"},
              {"id":"3p-odd","window":"monthly","remaining_fraction":0.5},
              {"id":"3p-none","window":"5h"}]}]}}}"#;
        let got = windows(Some(0), body, "a").unwrap();
        let shown: Vec<_> = got
            .iter()
            .map(|w| {
                (
                    w.name.as_str(),
                    w.group.as_deref().unwrap(),
                    w.used_percent,
                    w.resets_at,
                    w.minutes,
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                (
                    "gemini-weekly",
                    "Gemini Models",
                    25.0,
                    Some(1_796_090_291),
                    Some(10_080)
                ),
                (
                    "gemini-5h",
                    "Gemini Models",
                    100.0,
                    Some(1_795_521_491),
                    Some(300)
                ),
                ("3p-5h", "Claude and GPT models", 0.0, None, Some(300)),
                ("3p-odd", "Claude and GPT models", 50.0, None, None),
            ]
        );
    }

    #[test]
    fn a_failure_names_its_fix() {
        let out = r#"{"status":"ERROR","error":"authentication failed or timed out"}"#;
        let fault = windows(Some(1), out, "`HOME=/h agy` on mini").unwrap_err();
        assert!(
            fault.contains("exited 1: authentication failed or timed out")
                && fault.contains("log it in with `HOME=/h agy` on mini"),
            "{fault}"
        );
        let fault = windows(Some(2), "", "a").unwrap_err();
        assert!(
            fault.contains("exited 2: no error said: an API-key login has no account quota"),
            "{fault}"
        );
        let fault = windows(None, "", "a").unwrap_err();
        assert!(fault.contains("was killed"), "{fault}");
        let odd = r#"{"status":"SUCCESS","command":{"data":{"groups":[]}}}"#;
        let fault = windows(Some(0), odd, "a").unwrap_err();
        assert!(fault.contains("answered no window"), "{fault}");
        let fault = windows(Some(0), r#"{"status":"ERROR"}"#, "a").unwrap_err();
        assert!(fault.contains("try again later"), "{fault}");
    }
}
