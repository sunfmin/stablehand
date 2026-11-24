//! Pick: which account one more session may start on now (GLOSSARY.md
//! "Pick"). No I/O: the caller hands in each candidate, what the register
//! and the login say of it, and its books now.
//!
//! The rules are Guild's ledger's quota (`crates/ledger`: `quota`,
//! `Funds::stopped`, `Funds::runs_out`, `Funds::full`), carried over: an
//! account any of whose windows is the stop percent used takes no new
//! session until that window resets, those running go on; one past its
//! sessions at once takes none; one whose window, at its present rate with
//! the sessions running and one more, runs out before it resets takes none;
//! of the rest, the one with the most left is picked. A session of a model
//! is judged by the windows its model spends: an account whose windows are
//! grouped (agy's model groups) by those of the group holding the model, and
//! those in no group.
use crate::books::{self, Status, WindowStatus};
use serde::Serialize;

/// How much of any window an account may have used and still take a new
/// session, in percent, unless `--stop-percent` says (Guild's
/// `STOP_PERCENT`, Felix 2026-11-16).
pub const STOP_PERCENT: u8 = 90;

/// One account that may be picked, as the register, its login and its books
/// say of it now.
pub struct Candidate<'a> {
    pub name: &'a str,
    /// Why no session may start on it here, whatever its books say.
    pub barred: Option<(Reason, String)>,
    /// Sessions at once; none: no limit.
    pub sessions: Option<u32>,
    /// The sessions now running on it, as the caller counts them.
    pub running: u32,
    pub status: Status,
}

/// Why an account was passed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Switched off (`stablehand off`).
    Off,
    /// Its login is bound to another machine.
    Elsewhere,
    /// Not logged in.
    NotLoggedIn,
    /// A caller's limit holds on it.
    Limit,
    /// A window of it is at the stop percent.
    Stopped,
    /// Its sessions at once are running.
    Full,
    /// A window of it would run out before it resets with one more session.
    RunsOut,
}

/// An account passed over, why, and until when, where that is known.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Passed {
    pub account: String,
    pub reason: Reason,
    pub why: String,
    pub until: Option<u64>,
}

/// The answer: an account, or none, until the earliest time any passed over
/// may take one, and why each was passed over.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Account(String),
    None {
        until: Option<u64>,
        why: Vec<Passed>,
    },
}

/// Which of `candidates` one more session, of `model` where the caller
/// names it, may start on at `now`, an account whose window is
/// `stop_percent` used taking none: of those that may, the one with the
/// most left of its most used window; one with no reading in force (or only
/// stale) after every one with one; of equals, the first given. Only the
/// windows [`spent`] by `model` decide.
pub fn pick(candidates: &[Candidate], stop_percent: u8, model: Option<&str>, now: u64) -> Pick {
    // The most left so far; a later one replaces it only with more.
    let mut best: Option<(&Candidate, Option<f64>)> = None;
    let mut why = Vec::new();
    for c in candidates {
        let windows = spent(&c.status, model);
        match passed(c, &windows, stop_percent, now) {
            Some((reason, text, until)) => why.push(Passed {
                account: c.name.to_string(),
                reason,
                why: text,
                until,
            }),
            None => {
                let left = left_percent(&windows);
                if best.is_none_or(|(_, most)| left > most) {
                    best = Some((c, left));
                }
            }
        }
    }
    match best {
        Some((c, _)) => Pick::Account(c.name.to_string()),
        None => Pick::None {
            until: why.iter().filter_map(|p| p.until).min(),
            why,
        },
    }
}

/// The windows of `s` a session of `model` spends: of an account whose
/// windows are grouped, those in no group and those of each group one of
/// whose windows names the model; every window where none does
/// ([`in_no_group`]), or no model is named.
pub fn spent<'a>(s: &'a Status, model: Option<&str>) -> Vec<&'a WindowStatus> {
    let groups: Vec<&str> = s
        .windows
        .iter()
        .filter(|w| model.is_some_and(|m| w.models.iter().any(|id| id == m)))
        .filter_map(|w| w.group.as_deref())
        .collect();
    s.windows
        .iter()
        .filter(|w| groups.is_empty() || w.group.as_deref().is_none_or(|g| groups.contains(&g)))
        .collect()
}

/// Whether `s` groups its windows and no window of it names `model`: a
/// session of it is judged by every window.
pub fn in_no_group(s: &Status, model: &str) -> bool {
    s.windows.iter().any(|w| w.group.is_some())
        && !s
            .windows
            .iter()
            .any(|w| w.models.iter().any(|id| id == model))
}

/// How much of its most used window is left, 0 to 100, of `windows`
/// whose use is known now and not stale.
fn left_percent(windows: &[&WindowStatus]) -> Option<f64> {
    windows
        .iter()
        .filter(|w| !w.stale)
        .filter_map(|w| w.used_percent)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|used| (100.0 - used).max(0.0))
}

/// Why `c`, judged by `windows` of it, takes no new session at `now`, and
/// until when; none where it may.
fn passed(
    c: &Candidate,
    windows: &[&WindowStatus],
    stop_percent: u8,
    now: u64,
) -> Option<(Reason, String, Option<u64>)> {
    if let Some((reason, text)) = &c.barred {
        return Some((*reason, text.clone(), None));
    }
    let s = &c.status;
    // A limit of a window not judged holds on another group, not this one.
    if let Some(l) = s.limit.as_ref().filter(|l| {
        l.window
            .as_ref()
            .is_none_or(|name| windows.iter().any(|w| &w.name == name))
    }) {
        let until = match &l.window {
            Some(name) => s
                .windows
                .iter()
                .find(|w| &w.name == name)
                .and_then(|w| w.resets_at),
            None => l.resets_at,
        };
        let what = l
            .window
            .as_deref()
            .map_or("the account".into(), |w| format!("its {w}"));
        let lifts = until.map_or("its next reading".into(), crate::time::utc);
        return Some((
            Reason::Limit,
            format!("a session met {what}'s limit: it lifts at {lifts}"),
            until,
        ));
    }
    if let Some((w, used)) = windows
        .iter()
        .filter_map(|w| Some((w, w.used_percent?)))
        .filter(|(_, used)| *used >= f64::from(stop_percent))
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    {
        return Some((
            Reason::Stopped,
            format!(
                "its {} is {used}% used, at its stop of {stop_percent}%: no new session until it resets",
                w.name
            ),
            w.resets_at,
        ));
    }
    if let Some(limit) = c.sessions.filter(|&limit| c.running >= limit) {
        return Some((
            Reason::Full,
            format!(
                "{} sessions running, of its {limit} at once: one must end first",
                c.running
            ),
            None,
        ));
    }
    let (w, at_reset) = windows
        .iter()
        .filter_map(|w| Some((w, books::runs_out(w, c.running, now)?)))
        .min_by_key(|(w, _)| w.resets_at)?;
    Some((
        Reason::RunsOut,
        format!(
            "its {} is {}% used and, with {} running and one more, would be {at_reset:.0}% used at its reset: it runs out first",
            w.name,
            w.used_percent.unwrap_or_default(),
            c.running
        ),
        w.resets_at,
    ))
}

#[cfg(test)]
#[path = "pick_tests.rs"]
mod tests;
