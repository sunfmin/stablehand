//! Pick: which account one more session may start on now (GLOSSARY.md
//! "Pick"). No I/O: the caller hands in each candidate, what the register
//! and the login say of it, and its books now.
//!
//! The rules are Guild's ledger's quota (`crates/ledger`: `quota`,
//! `Funds::stopped`, `Funds::runs_out`), carried over: an
//! account any of whose windows is the stop percent used takes no new
//! session until that window resets, those running go on; one whose
//! window, at its present rate with
//! the sessions running and one more, runs out before it resets takes none,
//! its long window ([`long_window`]) left out of that rate. Of the rest, the
//! one whose long window resets first is picked, quota that expires soonest
//! spent first (docs/adr/0005); of equals, and of those whose reset is not
//! known, after them, the one with the most left. A session of a model is
//! judged by the books its model spends ([`spent`]).
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
    /// The sessions now running on it, as the caller counts them.
    pub running: u32,
    pub status: Status,
}

/// Why an account was passed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// No account of the engine is kept: nothing starts on a login stablehand does not keep.
    NoAccount,
    /// Switched off (`stablehand off`).
    Off,
    /// Its login is bound to another machine.
    Elsewhere,
    /// A copy of another kept account that is on here (GLOSSARY.md "Identity").
    Copy,
    /// Not logged in.
    NotLoggedIn,
    /// A caller's limit holds on it.
    Limit,
    /// A window of it is at the stop percent.
    Stopped,
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

/// Which of `candidates` one more session may start on at `now`, an
/// account whose window is `stop_percent` used taking none: of those that
/// may, the one whose long window resets first; of equals, and of those
/// whose reset is not known (after every one known), the one with the most
/// left of its most used window, one with no reading in force (or only
/// stale) after every one with one; of equals, the first given.
pub fn pick(candidates: &[Candidate], stop_percent: u8, now: u64) -> Pick {
    // The first so far; a later one replaces it only by going before it.
    let mut best: Option<(&Candidate, Rank)> = None;
    let mut why = Vec::new();
    for c in candidates {
        match passed(c, stop_percent, now) {
            Some((reason, text, until)) => why.push(Passed {
                account: c.name.to_string(),
                reason,
                why: text,
                until,
            }),
            None => {
                let rank = Rank {
                    resets_at: long_window(&c.status)
                        .and_then(|w| w.resets_at)
                        .filter(|at| *at > now),
                    left: left_percent(&c.status),
                };
                if best.is_none_or(|(_, first)| rank.goes_before(&first)) {
                    best = Some((c, rank));
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

/// Where an account stands in a step's ranking.
#[derive(Clone, Copy)]
struct Rank {
    /// When its long window resets, where known and still ahead.
    resets_at: Option<u64>,
    /// How much of its most used window is left ([`left_percent`]).
    left: Option<f64>,
}

impl Rank {
    /// Whether it is picked before `other`: the earlier reset, one known
    /// before none; else the most left, some before none.
    fn goes_before(&self, other: &Rank) -> bool {
        match (self.resets_at, other.resets_at) {
            (Some(a), Some(b)) if a != b => a < b,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            _ => self.left > other.left,
        }
    }
}

/// The window that holds an account's quota longest: its longest by length
/// (each reader gives the length its engine or the window's name says),
/// where it has a shorter one too. An account whose windows are all of one
/// length, or of none known, has none: a sole window may be the short one.
fn long_window(s: &Status) -> Option<&WindowStatus> {
    let longest = s
        .windows
        .iter()
        .filter(|w| w.minutes.is_some())
        .max_by_key(|w| w.minutes)?;
    let shortest = s.windows.iter().filter_map(|w| w.minutes).min()?;
    (longest.minutes > Some(shortest)).then_some(longest)
}

/// `s` as a session of `model` spends it: of an account whose windows are
/// grouped (agy's model groups), only those of the group holding the model
/// and those in no group, and a limit only on one of them or on the whole
/// account; and whether it groups its windows but none holds the model,
/// every window then kept.
pub fn spent(mut s: Status, model: &str) -> (Status, bool) {
    let groups: Vec<String> = s
        .windows
        .iter()
        .filter(|w| w.models.iter().any(|id| id == model))
        .filter_map(|w| w.group.clone())
        .collect();
    if groups.is_empty() {
        let in_no_group = s.windows.iter().any(|w| w.group.is_some());
        return (s, in_no_group);
    }
    s.windows
        .retain(|w| w.group.as_ref().is_none_or(|g| groups.contains(g)));
    let kept = |name: &String| s.windows.iter().any(|w| &w.name == name);
    if s.limit
        .as_ref()
        .and_then(|l| l.window.as_ref())
        .is_some_and(|name| !kept(name))
    {
        s.limit = None;
    }
    (s, false)
}

/// How much of its most used window is left, 0 to 100, of the windows
/// whose use is known now and not stale.
fn left_percent(s: &Status) -> Option<f64> {
    s.windows
        .iter()
        .filter(|w| !w.stale)
        .filter_map(|w| w.used_percent)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|used| (100.0 - used).max(0.0))
}

/// Why `c` takes no new session at `now`, and until when; none where it may.
fn passed(c: &Candidate, stop_percent: u8, now: u64) -> Option<(Reason, String, Option<u64>)> {
    if let Some((reason, text)) = &c.barred {
        return Some((*reason, text.clone(), None));
    }
    let s = &c.status;
    if let Some(l) = &s.limit {
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
    if let Some((w, used)) = s
        .windows
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
    // Running the long window down just before its reset is the aim; the
    // stop percent holds its margin.
    let long = long_window(s);
    let (w, at_reset) = s
        .windows
        .iter()
        .filter(|w| !long.is_some_and(|l| std::ptr::eq(*w, l)))
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
