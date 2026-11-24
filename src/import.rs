//! `account import --from-mytokens`: this host's `agent-account-*` entries of
//! mytokens moved into the register (docs/adr/0001 "One home"), a portable
//! one's token left in mytokens, read by the `mytokens get` the account
//! names as its token command. mytokens is only read, and no token is.
use crate::account::{self, Account, Change, Engine, Terms};
use crate::refusal::{Code, Refused};
use crate::store::Store;
use serde_json::{Value, json};
use std::process::{Command, Stdio};

const PREFIX: &str = "agent-account-";
const TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";
/// Where factorium's workers keep each bound account's home, under `$HOME`
/// (its harness's `HOMES`): a bound account is imported with that home.
const HOMES: &str = ".agent-accounts";

/// The home factorium's workers give the bound account `name` on this host.
fn workers_home(name: &str) -> Result<String, String> {
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .ok_or("$HOME is not set: run the import as the person whose accounts they are")?;
    Ok(format!("{home}/{HOMES}/{name}"))
}

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

    /// The token command of its account: `mytokens get` of its token, each
    /// word quoted for `sh -c`, where it has a token.
    fn token_command(&self) -> Option<String> {
        let quoted = self.get(TOKEN).map(|w| {
            let plain = w
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./@:".contains(c));
            if plain {
                w.to_string()
            } else {
                format!("'{}'", w.replace('\'', "'\\''"))
            }
        });
        self.has(TOKEN).then(|| quoted.join(" "))
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
    let home = match machine {
        Some(_) => Some(workers_home(e.name()).map_err(|f| vec![f])?),
        None => None,
    };
    let change = Change {
        terms: Terms {
            machine,
            home,
            token_command: e.token_command(),
            ..Terms::default()
        },
        ..Change::default()
    };
    account::new(e.name(), engine, change)
        .map_err(|faults| faults.into_iter().map(|f| format!("{id}: {f}")).collect())
}

/// What an import does with one entry.
#[derive(Clone, Copy, PartialEq)]
enum Step {
    Kept,
    Add,
    /// A kept bound account with no home is given the workers' one.
    GiveHome,
    /// A kept portable account naming no token command is given its entry's.
    GiveTokenCommand,
}

/// What `kept` lacks of what an import gives an account of entry `e`: a
/// bound one the workers' home, a portable one its token command.
fn step(kept: &Account, e: &Entry) -> Result<(Account, Step), String> {
    if kept.machine.is_some() && kept.home.is_none() {
        let home = Some(workers_home(&kept.name)?);
        return Ok((
            Account {
                home,
                ..kept.clone()
            },
            Step::GiveHome,
        ));
    }
    match e.token_command() {
        Some(cmd) if kept.machine.is_none() && kept.token_command.is_none() => Ok((
            Account {
                token_command: Some(cmd),
                ..kept.clone()
            },
            Step::GiveTokenCommand,
        )),
        _ => Ok((kept.clone(), Step::Kept)),
    }
}

/// Imports every entry not kept yet, and gives a kept account what an
/// import gives it where it lacks it ([`step`]; `dry_run`: says what it
/// would): all its entries' faults refuse it before anything is written.
pub fn import(store: &Store, dry_run: bool) -> Result<Value, Refused> {
    let list = read(&["mytokens", "list"])?;
    let mut faults = Vec::new();
    let mut plan = Vec::new();
    for e in entries(&list) {
        match store.get(e.name())? {
            Some(kept) => {
                let (a, step) = step(&kept, &e)?;
                plan.push((e, a, step));
            }
            None => match planned(&e) {
                Ok(a) => plan.push((e, a, Step::Add)),
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
    for (e, a, step) in &plan {
        let result = match (step, dry_run) {
            (Step::Kept, _) => "already kept",
            (Step::GiveHome, true) => "would give its home",
            (Step::GiveTokenCommand, true) => "would give its token command",
            (Step::Add, true) => "would add",
            (_, false) => {
                store.put(a)?;
                match step {
                    Step::Add => "added",
                    Step::GiveHome => "given its home",
                    _ => "given its token command",
                }
            }
        };
        done.push(json!({ "from": e.id, "result": result, "account": a.shown() }));
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
