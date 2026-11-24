//! `read`, `limit` and `status`: an account's readings booked through its
//! engine's reader, a caller's limit booked, and what the books say now.
use crate::account::Account;
use crate::books::{self, Entry, Status, What};
use crate::readers;
use crate::refusal::{Code, Refused};
use crate::store::Store;
use crate::time::utc;
use serde_json::{Value, json};

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
        let before = books::status(&store.entries(&a.name)?, now);
        let (outcome, said) = match (readers::of(a.engine), books::read_next(&before, now)) {
            (None, _) => ("no_reader", None),
            // A refused credential is not asked again, `--now` or not.
            (Some(_), _) if before.refused => {
                ("refused", before.failed.clone().map(|why| ("error", why)))
            }
            (Some(_), Some(next)) if !now_anyway => ("waits", Some(("next_at", utc(next)))),
            (Some(reader), _) => {
                let what = reader.read(a);
                let outcome = match &what {
                    What::Failed(why) => ("failed", Some(("error", why.clone()))),
                    What::Refused(why) => ("refused", Some(("error", why.clone()))),
                    _ => ("read", None),
                };
                store.book(&a.name, &Entry { at: now, what })?;
                outcome
            }
        };
        let after = books::status(&store.entries(&a.name)?, now);
        let mut shown = json!({ "account": a.name, "outcome": outcome, "windows": windows(&after), "balances": balances(&after) });
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
        let s = books::status(&store.entries(&a.name)?, now);
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
                "wallet": b.wallet,
                "currency": b.currency,
                "amount": b.amount,
                "at": utc(b.at),
                "stale": b.stale,
            })
        })
        .collect()
}
