//! `read`, `limit`, `status` and `pick`: an account's readings booked
//! through its engine's reader, a caller's limit booked, what the books say
//! now, and which account one more session may start on.
use crate::account::{Account, Engine};
use crate::books::{self, Entry, ReadBy, Status, What};
use crate::login;
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
fn book_of(store: &Store, a: &Account) -> Result<Vec<Entry>, String> {
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

/// Which of `accounts` of `engine` one more session may start on now on
/// `machine`: each is barred off, bound to another machine, or not logged
/// in, then judged by its books with `running` sessions on it
/// ([`pick::pick`]).
pub fn pick(
    store: &Store,
    accounts: &[Account],
    engine: Engine,
    running: &[(String, u32)],
    machine: &str,
    stop_percent: u8,
    now: u64,
) -> Result<Value, String> {
    let mut candidates = Vec::new();
    for a in accounts.iter().filter(|a| a.engine == engine) {
        let barred = match (&a.off, &a.machine) {
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
        };
        candidates.push(Candidate {
            name: &a.name,
            barred,
            sessions: a.sessions,
            running: running
                .iter()
                .filter(|(n, _)| n == &a.name)
                .map(|(_, c)| c)
                .sum(),
            status: books::status(&book_of(store, a)?, now),
        });
    }
    Ok(match pick::pick(&candidates, stop_percent, now) {
        Pick::Account(name) => json!({ "account": name }),
        Pick::None { until, why } => {
            let why: Vec<Value> = why
                .iter()
                .map(|p| {
                    json!({
                        "account": p.account,
                        "reason": p.reason,
                        "why": p.why,
                        "until": p.until.map(utc),
                    })
                })
                .collect();
            json!({ "none": true, "until": until.map(utc), "why": why })
        }
    })
}
