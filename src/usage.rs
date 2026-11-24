//! `read`, `limit` and `status`: an account's readings booked through its
//! engine's reader, a caller's limit booked, and what the books say now.
use crate::account::Account;
use crate::books::{self, Entry, Status, What};
use crate::readers;
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
            (Some(_), Some(next)) if !now_anyway => ("waits", Some(("next_at", utc(next)))),
            (Some(reader), _) => {
                let (what, outcome) = match reader(a) {
                    Ok(windows) => (What::Read(windows), ("read", None)),
                    Err(why) => (What::Failed(why.clone()), ("failed", Some(("error", why)))),
                };
                store.book(&a.name, &Entry { at: now, what })?;
                outcome
            }
        };
        let after = books::status(&store.entries(&a.name)?, now);
        let mut shown =
            json!({ "account": a.name, "outcome": outcome, "windows": windows(&after) });
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
) -> Result<Value, String> {
    let resets_at = match resets_at {
        None => None,
        Some(t) => {
            let at = crate::time::parse(&t).ok_or_else(|| {
                format!("--resets-at `{t}` is no time: give it as RFC 3339, 2026-11-24T15:00:00Z")
            })?;
            if at <= now {
                return Err(format!(
                    "--resets-at {t} is past: the limit has lifted; give when it lifts, or no --resets-at"
                ));
            }
            Some(at)
        }
    };
    if window.as_deref().is_some_and(|w| w.trim().is_empty()) {
        return Err("--window is empty: name the window the engine said ran out (five_hour), or leave it out".into());
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
            "limit": s.limit.as_ref().map(|l| json!({
                "at": utc(l.at),
                "window": l.window,
                "resets_at": l.resets_at.map(utc),
            })),
            "read_at": s.read_at.map(utc),
            "tried_at": s.tried_at.map(utc),
            "failed": s.failed,
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
