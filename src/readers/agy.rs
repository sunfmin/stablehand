//! agy's reader: `agy -p /usage --output-format json`, run in the account's
//! home (`HOME`), answers the quota with no model turn and no conversation
//! made. Its windows are `command.data.groups[].buckets[]`, a group per
//! model group: `id` (`gemini-5h`), `window` (`5h`, `weekly`),
//! `remaining_fraction` (used is 1 − it) and `reset_time`, UTC, 1970 for a
//! window not yet used. An API-key login exits 2: it has no account quota.
//! Each group's `description` names its models ("Models within this group:
//! Gemini 4 Neon, …"); `agy models`, run after it in the same home, lists
//! each model's id and name (`gemini-4-neon-high\tGemini 4 Neon (High)`), and
//! a window books the ids of its group's models: a name of the description,
//! or it with a variant after it (` (High)`).
use crate::account::Account;
use crate::books::Window;
use crate::bounded;
use crate::{login, time};
use std::process::{Command, Stdio};
use std::time::Duration;

/// How long agy may take to answer. A home it is not signed in to waits 60
/// seconds for a browser sign-in, then says so: this is past that.
const TIME_LIMIT: Duration = Duration::from_secs(90);
/// How long `agy models` may take, run once `/usage` has answered.
const MODELS_TIME_LIMIT: Duration = Duration::from_secs(30);

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
    let agy = |args: &[&str]| agy(&home, args);
    let (code, stdout) = run(
        agy(&["-p", "/usage", "--output-format", "json"]),
        TIME_LIMIT,
    )?;
    // Without the list, each window books no model: a pick of a model
    // judges the account by every window, and says so.
    let catalog = || match run(agy(&["models"]), MODELS_TIME_LIMIT) {
        Ok((Some(0), list)) => catalog(&list),
        _ => Vec::new(),
    };
    windows(code, &stdout, &login::login_line(account), catalog)
}

/// The ids `agy models` lists, run in `home` (`HOME`), or why it gave none,
/// with the fix.
pub fn models(home: &str) -> Result<Vec<String>, String> {
    match run(agy(home, &["models"]), MODELS_TIME_LIMIT)? {
        (Some(0), list) => Ok(catalog(&list).into_iter().map(|(id, _)| id).collect()),
        (code, _) => Err(format!(
            "`HOME={home} agy models` {}: sign agy in there (`HOME={home} agy`), then try again",
            code.map_or("was killed".into(), |c| format!("exited {c}"))
        )),
    }
}

/// agy run on `args` in `home` (`HOME`), no input, its stderr dropped.
fn agy(home: &str, args: &[&str]) -> Command {
    let mut agy = Command::new("agy");
    agy.args(args)
        .env("HOME", home)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    agy
}

/// Each model `agy models` lists, `(id, name)`, one a line, tab between.
fn catalog(list: &str) -> Vec<(String, String)> {
    list.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, name)| (id.trim().to_string(), name.trim().to_string()))
        .filter(|(id, name)| !id.is_empty() && !name.is_empty())
        .collect()
}

/// The ids of `catalog` whose name is one of `names` (a group's
/// description's), or one of them with a variant after it: `Gemini 4 Neon
/// (High)` is `Gemini 4 Neon`'s, `Orchestrator (Neon)` only itself.
fn ids(names: &[&str], catalog: &[(String, String)]) -> Vec<String> {
    catalog
        .iter()
        .filter(|(_, name)| {
            names.iter().any(|n| {
                name == n
                    || name
                        .strip_prefix(*n)
                        .is_some_and(|rest| rest.starts_with(" ("))
            })
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// The model names a group's `description` gives, after its colon, comma
/// separated.
fn names(description: &str) -> Vec<&str> {
    description
        .split_once(':')
        .map(|(_, list)| list)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect()
}

/// Runs `cmd` for its exit code and stdout, killed past `limit`.
fn run(cmd: Command, limit: Duration) -> Result<(Option<i32>, String), String> {
    bounded::answer(cmd, limit, "agy", "install agy, or put it on PATH")
}

/// The windows of agy's answer, its exit `code` and `stdout`, each with the
/// ids of its group's models in `catalog` (asked only of an answer with
/// groups), or why it gave none, with the fix.
fn windows(
    code: Option<i32>,
    stdout: &str,
    relog: &str,
    catalog: impl FnOnce() -> Vec<(String, String)>,
) -> Result<Vec<Window>, String> {
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
    // The list is asked once, of the first group.
    let catalog = std::cell::LazyCell::new(catalog);
    let mut found = Vec::new();
    for g in answer["command"]["data"]["groups"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let group = g["name"].as_str().map(str::to_string);
        let buckets = g["buckets"].as_array().into_iter().flatten();
        let windows: Vec<Window> = buckets
            .filter_map(|b| {
                let left = b["remaining_fraction"].as_f64()?;
                Some(Window {
                    name: b["id"].as_str()?.to_string(),
                    group: group.clone(),
                    models: Vec::new(),
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
            .collect();
        let models = ids(
            &names(g["description"].as_str().unwrap_or_default()),
            &catalog,
        );
        found.extend(windows.into_iter().map(|w| Window {
            models: models.clone(),
            ..w
        }));
    }
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
        let got = windows(Some(0), body, "a", Vec::new).unwrap();
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
    fn each_window_books_its_group_s_model_ids_by_their_names() {
        let body = r#"{"status":"SUCCESS","command":{"data":{"groups":[
            {"name":"Gemini Models","description":"Models within this group: Gemini 4 Neon, Orchestrator (Neon)","buckets":[
              {"id":"gemini-5h","window":"5h","remaining_fraction":0}]},
            {"name":"Claude and GPT models","description":"Models within this group: Claude Opus 5.5, GPT-OSS 120B","buckets":[
              {"id":"3p-5h","window":"5h","remaining_fraction":1},
              {"id":"3p-weekly","window":"weekly","remaining_fraction":1}]},
            {"name":"Odd","buckets":[{"id":"odd-5h","window":"5h","remaining_fraction":1}]}]}}}"#;
        let list = "gemini-4-neon-high\tGemini 4 Neon (High)\n\
            gemini-4-neon-pro\tGemini 4 Neon Pro (High)\n\
            gemini-pro-orchestrator-neon\tOrchestrator (Neon)\n\
            gemini-pro-orchestrator\tOrchestrator (Carbon)\n\
            claude-opus-5-5-high\tClaude Opus 5.5 (High)\n\
            claude-opus-5-5-low\t Claude Opus 5.5 (Low) \n\
            gpt-oss-120b-medium\tGPT-OSS 120B (Medium)\n\
            no tab here\n\
            \tClaude Opus 5.5 (Max)\n";
        let got = windows(Some(0), body, "a", || catalog(list)).unwrap();
        let models: Vec<_> = got
            .iter()
            .map(|w| (w.name.as_str(), w.models.clone()))
            .collect();
        let gemini = vec![
            "gemini-4-neon-high".to_string(),
            "gemini-pro-orchestrator-neon".into(),
        ];
        let third = vec![
            "claude-opus-5-5-high".to_string(),
            "claude-opus-5-5-low".into(),
            "gpt-oss-120b-medium".into(),
        ];
        assert_eq!(
            models,
            [
                ("gemini-5h", gemini),
                ("3p-5h", third.clone()),
                ("3p-weekly", third),
                ("odd-5h", vec![]),
            ]
        );
    }

    #[test]
    fn a_failure_names_its_fix() {
        let out = r#"{"status":"ERROR","error":"authentication failed or timed out"}"#;
        let fault = windows(Some(1), out, "`HOME=/h agy` on mini", Vec::new).unwrap_err();
        assert!(
            fault.contains("exited 1: authentication failed or timed out")
                && fault.contains("log it in with `HOME=/h agy` on mini"),
            "{fault}"
        );
        let fault = windows(Some(2), "", "a", Vec::new).unwrap_err();
        assert!(
            fault.contains("exited 2: no error said: an API-key login has no account quota"),
            "{fault}"
        );
        let fault = windows(None, "", "a", Vec::new).unwrap_err();
        assert!(fault.contains("was killed"), "{fault}");
        let odd = r#"{"status":"SUCCESS","command":{"data":{"groups":[]}}}"#;
        let fault = windows(Some(0), odd, "a", Vec::new).unwrap_err();
        assert!(fault.contains("answered no window"), "{fault}");
        let fault = windows(Some(0), r#"{"status":"ERROR"}"#, "a", Vec::new).unwrap_err();
        assert!(fault.contains("try again later"), "{fault}");
    }
}
