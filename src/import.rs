//! `account import --from-mytokens`: this host's `agent-account-*` entries of
//! mytokens moved into the register (docs/adr/0001 "One home"), each token
//! into sesame as `account login` keeps one. mytokens is only read.
use crate::account::{self, Account, Change, Engine, Terms};
use crate::login;
use crate::refusal::{Code, Refused};
use crate::store::Store;
use serde_json::{Value, json};
use std::process::{Command, Stdio};

const PREFIX: &str = "agent-account-";
const TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// One `agent-account-*` entry as a line of `mytokens list` lists it.
#[derive(Debug, PartialEq)]
struct Entry<'a> {
    /// `<service>/<label>`.
    id: &'a str,
    service: &'a str,
    label: &'a str,
    /// Its fields, `F, F`.
    fields: &'a str,
}

impl Entry<'_> {
    fn name(&self) -> &str {
        &self.service[PREFIX.len()..]
    }

    fn has(&self, field: &str) -> bool {
        self.fields.split(", ").any(|f| f == field)
    }

    /// `mytokens get` of `field`, as its argv.
    fn get<'f>(&'f self, field: &'f str) -> [&'f str; 7] {
        let (s, l) = (self.service, self.label);
        ["mytokens", "get", s, "--account", l, "--field", field]
    }
}

/// The `agent-account-*` entries of `mytokens list`'s lines,
/// `<service>/<label>\t<kind>\t<level>  — <about>  fields: <F>, <F>`.
fn entries(list: &str) -> impl Iterator<Item = Entry<'_>> {
    list.lines().filter_map(|line| {
        let (id, _) = line.split_once('\t')?;
        let (service, label) = id.split_once('/')?;
        service.strip_prefix(PREFIX).filter(|n| !n.is_empty())?;
        let fields = line.rsplit_once("  fields: ").map_or("", |(_, f)| f);
        Some(Entry {
            id,
            service,
            label,
            fields,
        })
    })
}

/// What `argv` (a mytokens call) printed, trimmed, or the fault naming it.
fn read(argv: &[&str]) -> Result<String, String> {
    let out = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| {
            format!(
                "`{}` could not start: {e}: install mytokens, or put it on PATH",
                argv[0]
            )
        })?;
    if !out.status.success() {
        return Err(format!("`{}` failed, as it said above", argv.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The account `e` makes, from its HARNESS and MACHINE, or its faults.
fn planned(e: &Entry) -> Result<Account, Vec<String>> {
    let id = e.id;
    let harness = read(&e.get("HARNESS")).map_err(|f| vec![f])?;
    let Ok(engine) = <Engine as clap::ValueEnum>::from_str(&harness, false) else {
        let engines = <Engine as clap::ValueEnum>::value_variants()
            .iter()
            .map(|e| e.name());
        return Err(vec![format!(
            "{id}: HARNESS `{harness}` is no engine ({}): `mytokens put {} --set HARNESS=<engine>`, or add the account by hand",
            engines.collect::<Vec<_>>().join(", "),
            e.service
        )]);
    };
    let machine = match e.has("MACHINE") {
        true => Some(read(&e.get("MACHINE")).map_err(|f| vec![f])?),
        false => None,
    };
    if machine.is_some() && e.has(TOKEN) {
        return Err(vec![format!(
            "{id}: a token and a MACHINE, and a bound login has no token: drop one of them in mytokens, or add the account by hand"
        )]);
    }
    let change = Change {
        terms: Terms {
            machine,
            ..Terms::default()
        },
        ..Change::default()
    };
    account::new(e.name(), engine, change)
        .map_err(|faults| faults.into_iter().map(|f| format!("{id}: {f}")).collect())
}

/// Imports every entry not kept yet (`dry_run`: says what it would): all its
/// entries' faults refuse it before anything is written.
pub fn import(store: &Store, dry_run: bool) -> Result<Value, Refused> {
    let list = read(&["mytokens", "list"])?;
    let mut faults = Vec::new();
    let mut plan = Vec::new();
    for e in entries(&list) {
        match store.get(e.name())? {
            Some(kept) => plan.push((e, kept, false)),
            None => match planned(&e) {
                Ok(a) => plan.push((e, a, true)),
                Err(f) => faults.extend(f),
            },
        }
    }
    if !faults.is_empty() {
        return Err(Refused::new(
            Code::ImportRefused,
            format!("nothing was imported: {}", faults.join("; ")),
        ));
    }
    let mut done = Vec::new();
    for (e, a, new) in &plan {
        let token = *new && e.has(TOKEN);
        let result = match (new, dry_run) {
            (false, _) => "already kept",
            (true, true) => "would add",
            (true, false) => {
                if token {
                    let again =
                        "; then `stablehand account import --from-mytokens` again imports the rest";
                    login::keep_token_from(a, &e.get(TOKEN)).map_err(|f| format!("{f}{again}"))?;
                }
                store.insert(a)?;
                "added"
            }
        };
        done.push(json!({ "from": e.id, "result": result, "token": token, "account": a.shown() }));
    }
    Ok(json!({ "dry_run": dry_run, "accounts": done }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_the_agent_account_lines_with_their_fields() {
        let list = "agent-account-codex/default\tprofile\tserver  — fields: as about  fields: HARNESS, MACHINE\n\
                    bark/default\tstatic\tserver  — push\n\
                    agent-account-/default\tprofile\tserver  — no name  fields: HARNESS\n\
                    agent-account-x/work\tprofile\tpersonal  — none";
        let all: Vec<_> = entries(list).collect();
        assert_eq!(
            all,
            [
                Entry {
                    id: "agent-account-codex/default",
                    service: "agent-account-codex",
                    label: "default",
                    fields: "HARNESS, MACHINE",
                },
                Entry {
                    id: "agent-account-x/work",
                    service: "agent-account-x",
                    label: "work",
                    fields: "",
                },
            ]
        );
        assert!(all[0].has("MACHINE") && !all[0].has("HARNESS, MACHINE"));
        assert_eq!(all[1].name(), "x");
    }
}
