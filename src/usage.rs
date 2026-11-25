//! `read`, `limit`, `status` and `pick`: an account's readings booked
//! through its engine's reader, a caller's limit booked, what the books say
//! now, and which account one more session may start on.
use crate::account::Account;
use crate::books::{self, Entry, ReadBy, Status, What};
use crate::login;
use crate::order::Step;
use crate::pick::{self, Candidate, Pick, Reason};
use crate::readers;
use crate::refusal::{Code, Refused};
use crate::store::Store;
use crate::time::utc;
use serde_json::{Value, json};

/// `a`'s books, with its login as an entry ([`What::LoggedIn`]) where its
/// home's login file says when it logged in: a credential refused before it
/// was written is asked again. It goes first, so one written in the second
/// of a refusal is not after it (the books keep the order of a second).
pub fn book_of(store: &Store, a: &Account) -> Result<Vec<Entry>, String> {
    let login = login::login_file(a)?.map(|f| Entry {
        at: f.written,
        what: What::LoggedIn,
    });
    Ok(login.into_iter().chain(store.entries(&a.name)?).collect())
}

/// Reads each of `accounts` whose engine has a reader and books what came
/// of it; one read in the last hour waits unless `now_anyway`.
pub fn read(
    store: &Store,
    accounts: &[Account],
    now: u64,
    now_anyway: bool,
) -> Result<Value, String> {
    let mut reads = Vec::new();
    for a in accounts {
        let before = books::status(&book_of(store, a)?, now);
        let (outcome, said) = match (readers::of(a.engine), books::read_next(&before, now)) {
            (None, _) => ("no_reader", None),
            // A refused credential is not asked again, `--now` or not.
            (Some(_), _) if before.refused => {
                ("refused", before.failed.clone().map(|why| ("error", why)))
            }
            (Some(_), Some(next)) if !now_anyway => ("waits", Some(("next_at", utc(next)))),
            (Some(reader), _) => {
                let what = reader(a, &before);
                let outcome = match &what {
                    What::Failed(why) => ("failed", Some(("error", why.clone()))),
                    What::Refused(why) => ("refused", Some(("error", why.clone()))),
                    _ => ("read", None),
                };
                store.book(&a.name, &Entry { at: now, what })?;
                outcome
            }
        };
        let after = books::status(&book_of(store, a)?, now);
        let mut shown = json!({ "account": a.name, "outcome": outcome, "windows": windows(&after), "balances": balances(&after) });
        if outcome == "read" {
            shown["read_by"] = json!(after.read_by);
            shown["spent_turn"] = json!(after.read_by == Some(ReadBy::Question));
        }
        if let Some((key, value)) = said {
            shown[key] = json!(value);
        }
        reads.push(shown);
    }
    Ok(json!({ "reads": reads }))
}

/// Books a caller's limit on `account` at `now`.
pub fn limit(
    store: &Store,
    account: &Account,
    window: Option<String>,
    resets_at: Option<String>,
    now: u64,
) -> Result<Value, Refused> {
    let bad = |text: String| Refused::new(Code::BadLimit, text);
    let resets_at = resets_at
        .map(|t| match crate::time::parse(&t) {
            None => Err(bad(format!(
                "--resets-at `{t}` is no time: give it as RFC 3339, 2026-11-24T15:00:00Z"
            ))),
            Some(at) if at <= now => Err(bad(format!(
                "--resets-at {t} is past: the limit has lifted; give when it lifts, or no --resets-at"
            ))),
            Some(at) => Ok(at),
        })
        .transpose()?;
    if window.as_deref().is_some_and(|w| w.trim().is_empty()) {
        return Err(bad(
            "--window is empty: name the window the engine said ran out (five_hour), or leave it out".into(),
        ));
    }
    store.book(
        &account.name,
        &Entry {
            at: now,
            what: What::Limit {
                window: window.clone(),
                resets_at,
            },
        },
    )?;
    Ok(json!({ "limit": {
        "account": account.name,
        "at": utc(now),
        "window": window,
        "resets_at": resets_at.map(utc),
    }}))
}

/// What the books say of each of `accounts` now.
pub fn status(store: &Store, accounts: &[Account], now: u64) -> Result<Value, String> {
    let mut shown = Vec::new();
    for a in accounts {
        let s = books::status(&book_of(store, a)?, now);
        shown.push(json!({
            "name": a.name,
            "engine": a.engine,
            "off": a.off.is_some(),
            "pace": s.pace,
            "windows": windows(&s),
            "balances": balances(&s),
            "limit": s.limit.as_ref().map(|l| json!({
                "at": utc(l.at),
                "window": l.window,
                "resets_at": l.resets_at.map(utc),
            })),
            "read_at": s.read_at.map(utc),
            "read_by": s.read_by,
            "tried_at": s.tried_at.map(utc),
            "failed": s.failed,
            "refused": s.refused,
            "read_next": books::read_next(&s, now).map(utc),
        }));
    }
    Ok(json!({ "accounts": shown }))
}

fn windows(s: &Status) -> Vec<Value> {
    s.windows
        .iter()
        .map(|w| {
            json!({
                "name": w.name,
                "group": w.group,
                "models": w.models,
                "used_percent": w.used_percent,
                "resets_at": w.resets_at.map(utc),
                "source": w.source,
                "at": utc(w.at),
                "stale": w.stale,
                "pace": w.pace,
            })
        })
        .collect()
}

fn balances(s: &Status) -> Vec<Value> {
    s.balances
        .iter()
        .map(|b| {
            json!({
                "wallet": b.balance.wallet,
                "currency": b.balance.currency,
                "amount": b.balance.amount,
                "at": utc(b.at),
                "stale": b.stale,
            })
        })
        .collect()
}

/// The session one more pick is for, as its caller says it.
pub struct Session<'a> {
    /// The sessions now running per account.
    pub running: &'a [(String, u32)],
    /// The machine it starts on.
    pub machine: &'a str,
}

/// What the session starts on: for each step of `walk` in turn, the
/// accounts of its engine, each barred off, bound to another machine, or
/// not logged in, then judged by its books with the sessions running on it
/// ([`pick::pick`]), of a model by the books it spends ([`pick::spent`]);
/// the first (engine, model, account) that may, or none, until the
/// earliest any passed over lifts. Either says why each step before it was
/// passed over (an engine with no account kept, skipped) and the accounts
/// judged by every window, no group of theirs holding the step's model.
pub fn pick(
    store: &Store,
    accounts: &[Account],
    walk: &[Step],
    session: &Session,
    stop_percent: u8,
    now: u64,
) -> Result<Value, String> {
    let mut why = Vec::new();
    let mut until: Option<u64> = None;
    let mut in_no_group: Vec<&str> = Vec::new();
    for step in walk {
        let of: Vec<&Account> = accounts
            .iter()
            .filter(|a| a.engine == step.engine)
            .collect();
        let passed = |account: Option<&str>, reason: Reason, text: &str, until: Option<u64>| {
            json!({
                "engine": step.engine,
                "model": step.model,
                "account": account,
                "reason": reason,
                "why": text,
                "until": until.map(utc),
            })
        };
        if of.is_empty() {
            let engine = step.engine.name();
            why.push(passed(None, Reason::NoAccount, &format!(
                "no {engine} account is kept: skipped; add one with `stablehand account add <name> --engine {engine}`, or `stablehand account scan --add`"
            ), None));
            continue;
        }
        let mut candidates = Vec::new();
        for a in of {
            let (status, whole) = judged(store, a, step.model.as_deref(), now)?;
            if whole && !in_no_group.contains(&a.name.as_str()) {
                in_no_group.push(&a.name);
            }
            candidates.push(Candidate {
                name: &a.name,
                barred: barred(a, session.machine)?,
                running: session
                    .running
                    .iter()
                    .filter(|(n, _)| n == &a.name)
                    .map(|(_, c)| c)
                    .sum(),
                status,
            });
        }
        match pick::pick(&candidates, stop_percent, now) {
            Pick::Account(name) => {
                return Ok(json!({
                    "engine": step.engine,
                    "model": step.model,
                    "account": name,
                    "why": why,
                    "model_in_no_group": in_no_group,
                }));
            }
            Pick::None {
                why: each,
                until: lifts,
            } => {
                until = until.into_iter().chain(lifts).min();
                for p in each {
                    why.push(passed(Some(&p.account), p.reason, &p.why, p.until));
                }
            }
        }
    }
    Ok(json!({
        "none": true,
        "until": until.map(utc),
        "why": why,
        "model_in_no_group": in_no_group,
    }))
}

/// Why no session may start on `a` on `machine`, whatever its books say.
fn barred(a: &Account, machine: &str) -> Result<Option<(Reason, String)>, String> {
    Ok(match (&a.off, &a.machine) {
        (Some(off), _) => Some((Reason::Off, format!("switched off: {}", off.reason))),
        (None, Some(bound)) if bound != machine => Some((
            Reason::Elsewhere,
            format!("its login is bound to {bound}, not {machine}"),
        )),
        _ if !login::logged_in(a)? => Some((
            Reason::NotLoggedIn,
            format!("not logged in: log it in with {}", login::login_line(a)),
        )),
        _ => None,
    })
}

/// `a`'s books now, as a session of `model` spends them, and whether it
/// is judged by every window, no group of its holding the model.
fn judged(
    store: &Store,
    a: &Account,
    model: Option<&str>,
    now: u64,
) -> Result<(Status, bool), String> {
    let status = books::status(&book_of(store, a)?, now);
    Ok(match model {
        Some(model) => pick::spent(status, model),
        None => (status, false),
    })
}

/// What a pick is asked: the engines and model it is narrowed to, the
/// sessions the caller counts running, its machine and stop percent.
pub struct Ask {
    pub engines: Vec<crate::account::Engine>,
    pub model: Option<String>,
    pub running: Vec<(String, u32)>,
    /// None: this machine.
    pub machine: Option<String>,
    pub stop_percent: u8,
}

/// The pick among `accounts` ([`pick`]) of the order's walk narrowed as
/// `ask` says, the sessions `run` keeps counted beside the caller's
/// (one whose engine's pid is dead is dropped first). Refused
/// (`no_order`) a walk with nothing in it.
pub fn pick_now(store: &Store, accounts: &[Account], ask: Ask, now: u64) -> Result<Value, Refused> {
    let machine = match ask.machine {
        Some(m) => m,
        None => login::this_machine()?,
    };
    let walk = crate::order::walk(&store.order()?, &ask.engines, ask.model.as_deref());
    if walk.is_empty() {
        return Err(Refused::new(
            Code::NoOrder,
            format!(
                "nothing to walk: {}; set the order, `stablehand order set claude-code@opus codex`, or name `--engine <engine>`",
                ask.model.map_or(
                    "no order is kept and no --engine named".into(),
                    |m| format!(
                        "no engine of the order lists model `{m}` and no --engine is named"
                    )
                )
            ),
        ));
    }
    let mut running = ask.running;
    for (pid, account) in store.sessions()? {
        if crate::run::alive(pid) {
            running.push((account, 1));
        } else {
            store.end_session(pid)?;
        }
    }
    let session = Session {
        running: &running,
        machine: &machine,
    };
    Ok(pick(
        store,
        accounts,
        &walk,
        &session,
        ask.stop_percent,
        now,
    )?)
}
