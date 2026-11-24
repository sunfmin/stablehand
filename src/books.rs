//! The books: what each reading, failed try and limit said of one account,
//! and what they say of it now (GLOSSARY.md "Reading", "Window", "Balance",
//! "Limit", "Pace"). No I/O: the store hands in an account's entries, the caller the
//! clock.
//!
//! The rules are Guild's ledger's (`crates/ledger`), carried over: a
//! window's use and reset are the engine's, as it last read them; a limit
//! uses up the window it names until it lifts; a window's pace is its use
//! run on at its recent rate against what is left before it resets; an
//! account is read again at most once an hour.
use serde::{Deserialize, Serialize};

/// One window of an account as its engine's reader turned it out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// The engine's own name for it: `five_hour`, `primary`, `limit_5h`.
    pub name: String,
    /// The group the engine puts it in (agy's model group, Grok's
    /// product); none where the engine groups none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// How much of it is used, 0 to 100.
    pub used_percent: f64,
    /// When it resets, in seconds since the Unix epoch, where the engine says.
    pub resets_at: Option<u64>,
    /// How long it is, in minutes, where the reader knows.
    pub minutes: Option<u64>,
}

/// One wallet's balance of a pay-as-you-go account (dsh) as its reader
/// turned it out (GLOSSARY.md "Balance").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Balance {
    /// The engine's wallet it is: dsh's `normal` (topped up) or `bonus`.
    pub wallet: String,
    /// Its currency, as the engine names it: `CNY`, `USD`.
    pub currency: String,
    /// What is left in it.
    pub amount: f64,
}

/// What one entry of the books says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum What {
    /// A reading: the windows the engine answered.
    Read(Vec<Window>),
    /// A reading of a pay-as-you-go account: its wallets' balances, no window.
    Balance(Vec<Balance>),
    /// A try to read that failed, and why.
    Failed(String),
    /// A try the engine refused the account's credential on, and why: the
    /// account is not read again until it logs in again ([`What::LoggedIn`]).
    Refused(String),
    /// The account logged in again (`stablehand account login`): a
    /// credential refused before it is asked again.
    LoggedIn,
    /// A caller's report that a session met the account's limit: of the
    /// window it names, lifting when it says, where it knows.
    Limit {
        window: Option<String>,
        resets_at: Option<u64>,
    },
}

/// One entry of an account's books, at `at` (seconds since the Unix epoch).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub at: u64,
    pub what: What,
}

/// Where a window's use and reset come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A reading.
    Read,
    /// A caller's `limit`: used up until it resets.
    Limit,
}

/// A window's pace, or an account's, the worst of its windows'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pace {
    /// Used up, or a limit holds: nothing more runs on it until it resets.
    UsedUp,
    /// At its recent rate it runs out before it resets.
    Tight,
    /// It resets within a quarter of its length with [`SPARE_PERCENT`] or
    /// more of it left at its rate: unused, it is lost.
    Spare,
    /// Neither, or it cannot be told.
    Steady,
}

/// One window of an account, now.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowStatus {
    pub name: String,
    pub group: Option<String>,
    /// How much of it is used, 0 to 100; none once it has reset since it
    /// was read.
    pub used_percent: Option<f64>,
    pub resets_at: Option<u64>,
    pub minutes: Option<u64>,
    /// How fast it is being used, in percent an hour, where two or more
    /// readings of it in force tell ([`recent_rate`]).
    pub rate: Option<f64>,
    pub source: Source,
    /// When the reading or the limit it comes from was booked.
    pub at: u64,
    /// A later try to read failed, or it has reset since it was read: its
    /// use is not known now.
    pub stale: bool,
    pub pace: Pace,
}

/// One wallet of an account, as last read.
#[derive(Debug, Clone, PartialEq)]
pub struct BalanceStatus {
    pub balance: Balance,
    /// When the reading it comes from was booked.
    pub at: u64,
    /// A later try to read failed: what is left now is not known.
    pub stale: bool,
}

/// A caller's limit while it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub at: u64,
    pub window: Option<String>,
    pub resets_at: Option<u64>,
}

/// One account's books, now.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub windows: Vec<WindowStatus>,
    /// Its wallets, where its engine reads a balance and no window.
    pub balances: Vec<BalanceStatus>,
    /// The worst of its windows' paces; used up while a limit holds.
    pub pace: Pace,
    pub limit: Option<Held>,
    /// When it was last read, and last tried, whatever came of it.
    pub read_at: Option<u64>,
    pub tried_at: Option<u64>,
    /// Why the last try failed, where it came after the last reading.
    pub failed: Option<String>,
    /// The last try's credential was refused and the account has not logged
    /// in since: it is not read until it does.
    pub refused: bool,
}

/// How much of a window must be left at its reset, at its rate, for it to
/// be spare.
pub const SPARE_PERCENT: f64 = 20.0;
/// How old a reading may be and still on its own make a window run out
/// before it resets, in seconds: two hours.
pub const REPORT_FRESH: u64 = 2 * 3600;
/// How much of a window must be used before its rate says it runs out, in
/// percent: under it a night's burst run on over the rest of a week is no
/// verdict.
pub const TIGHT_FROM_PERCENT: f64 = 50.0;
/// How long a reading stands before the account is read again, and how
/// long one try waits before the next, in seconds: an hour.
pub const READ_AGAIN: u64 = 3600;
/// The least span a window's recent rate is taken over, as a part of the
/// window: a twentieth, fifteen minutes of five hours.
pub const RATE_SPAN_PARTS: u64 = 20;

/// An account's books at `now`, from its entries in any order. An entry
/// booked after `now` is not yet in them.
pub fn status(entries: &[Entry], now: u64) -> Status {
    let mut book: Vec<&Entry> = entries.iter().filter(|e| e.at <= now).collect();
    book.sort_by_key(|e| e.at);
    // In the order booked: an entry of the same second as another, booked
    // after it, is later.
    let reading = book
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, e)| match &e.what {
            What::Read(windows) => Some((i, e.at, windows)),
            _ => None,
        });
    let wallets = book.iter().rev().find_map(|e| match &e.what {
        What::Balance(balances) => Some((e.at, balances)),
        _ => None,
    });
    let tried = book.iter().rev().find(|e| {
        matches!(
            e.what,
            What::Read(_) | What::Balance(_) | What::Failed(_) | What::Refused(_)
        )
    });
    let failed = tried.and_then(|e| match &e.what {
        What::Failed(why) | What::Refused(why) => Some(why.clone()),
        _ => None,
    });
    let refused = book
        .iter()
        .rev()
        .find(|e| !matches!(e.what, What::Limit { .. }))
        .is_some_and(|e| matches!(e.what, What::Refused(_)));
    let balances = wallets
        .map(|(at, balances)| {
            balances
                .iter()
                .map(|b| BalanceStatus {
                    balance: b.clone(),
                    at,
                    stale: failed.is_some(),
                })
                .collect()
        })
        .unwrap_or_default();
    let limit = held(&book, now);
    let mut windows: Vec<WindowStatus> = reading
        .map(|(i, at, read)| {
            let limit = limit.as_ref().filter(|(l, _)| *l > i).map(|(_, l)| l);
            read.iter()
                .map(|w| window_status(w, at, failed.is_some(), &book, limit, now))
                .collect()
        })
        .unwrap_or_default();
    let limit = limit.map(|(_, l)| l);
    if let Some(l) = &limit
        && let Some(name) = &l.window
        && !windows.iter().any(|w| &w.name == name)
    {
        windows.push(WindowStatus {
            name: name.clone(),
            group: None,
            used_percent: Some(100.0),
            resets_at: l.resets_at,
            minutes: None,
            rate: None,
            source: Source::Limit,
            at: l.at,
            stale: false,
            pace: Pace::UsedUp,
        });
    }
    let pace = if limit.is_some() {
        Pace::UsedUp
    } else {
        windows.iter().map(|w| w.pace).min().unwrap_or(Pace::Steady)
    };
    Status {
        windows,
        balances,
        pace,
        limit,
        read_at: reading.map(|(_, at, _)| at).max(wallets.map(|(at, _)| at)),
        tried_at: tried.map(|e| e.at),
        failed,
        refused,
    }
}

/// The latest limit, while it holds: until the reset it names, otherwise
/// until a later reading; with where it is in `book`.
fn held(book: &[&Entry], now: u64) -> Option<(usize, Held)> {
    let (i, at, window, resets_at) =
        book.iter()
            .enumerate()
            .rev()
            .find_map(|(i, e)| match &e.what {
                What::Limit { window, resets_at } => Some((i, e.at, window, *resets_at)),
                _ => None,
            })?;
    let holds = match resets_at {
        Some(resets_at) => resets_at > now,
        None => !book[i + 1..]
            .iter()
            .any(|later| matches!(later.what, What::Read(_))),
    };
    let window = window.clone();
    holds.then_some((
        i,
        Held {
            at,
            window,
            resets_at,
        },
    ))
}

/// Window `w` of the reading booked at `at`, `failed` whether a try failed
/// after it, `limit` the one booked after it, while it holds.
fn window_status(
    w: &Window,
    at: u64,
    failed: bool,
    book: &[&Entry],
    limit: Option<&Held>,
    now: u64,
) -> WindowStatus {
    let mut status = WindowStatus {
        name: w.name.clone(),
        group: w.group.clone(),
        used_percent: Some(w.used_percent),
        resets_at: w.resets_at,
        minutes: w.minutes,
        rate: None,
        source: Source::Read,
        at,
        stale: failed,
        pace: Pace::Steady,
    };
    if let Some(l) = limit
        && l.window.as_deref() == Some(w.name.as_str())
    {
        status.used_percent = Some(100.0);
        status.resets_at = l.resets_at.or(w.resets_at).filter(|r| *r > now);
        status.source = Source::Limit;
        status.at = l.at;
        status.stale = false;
        status.pace = Pace::UsedUp;
        return status;
    }
    // A reading holds until the reset it says, or, saying none, for the
    // window's length.
    let ends = w
        .resets_at
        .or_else(|| w.minutes.map(|m| at.saturating_add(m * 60)));
    if ends.is_some_and(|ends| ends <= now) {
        status.used_percent = None;
        status.resets_at = None;
        status.stale = true;
        return status;
    }
    status.rate = recent_rate(&w.name, w.resets_at, w.minutes, book);
    status.pace = window_pace(&status, now);
    status
}

/// How fast `name` is being used, in percent an hour, from the readings of
/// it in force (inside the window that resets at `resets_at`): the change
/// between the latest reading and the latest one at least
/// [`RATE_SPAN_PARTS`]th of the window before it (else the earliest), over
/// their time apart, taken as at least that span, so that one noisy pair
/// does not swing it. None with fewer than two readings, or no reset or
/// length known: the pace then takes the window's use since it opened.
fn recent_rate(
    name: &str,
    resets_at: Option<u64>,
    minutes: Option<u64>,
    book: &[&Entry],
) -> Option<f64> {
    let span = minutes? * 60;
    let opened = resets_at?.saturating_sub(span);
    let readings: Vec<(u64, f64)> = book
        .iter()
        .filter(|e| e.at >= opened)
        .filter_map(|e| match &e.what {
            What::Read(windows) => windows
                .iter()
                .find(|w| w.name == name)
                .map(|w| (e.at, w.used_percent)),
            _ => None,
        })
        .collect();
    let (&(last_at, last_used), earlier) = readings.split_last()?;
    let least = span / RATE_SPAN_PARTS;
    let &(first_at, first_used) = earlier
        .iter()
        .rev()
        .find(|(at, _)| last_at.saturating_sub(*at) >= least)
        .or_else(|| earlier.first())?;
    let apart = last_at.saturating_sub(first_at).max(least).max(1) as f64;
    Some(((last_used - first_used).max(0.0) * 3600.0 / apart).max(0.0))
}

/// How much of a window is used at its reset, were its present rate to run
/// on to it: none where its use, reset or length is not known. The present
/// rate is the recent one where readings tell it; otherwise its use over
/// the time it has been open, taken over at least a tenth of it. A window
/// under [`TIGHT_FROM_PERCENT`] used, or read longer than [`REPORT_FRESH`]
/// ago, runs on at no rate. `share`: how many times over that rate runs on
/// (1: as it is).
fn at_reset(w: &WindowStatus, now: u64, share: f64) -> Option<f64> {
    let (used, resets_at, minutes) = (w.used_percent?, w.resets_at?, w.minutes?);
    if resets_at <= now {
        return None;
    }
    let span = (minutes * 60) as f64;
    let left = resets_at.saturating_sub(now) as f64;
    let stale = now.saturating_sub(w.at) > REPORT_FRESH;
    let per_second = if stale || used < TIGHT_FROM_PERCENT {
        0.0
    } else if let Some(rate) = w.rate {
        rate / 3600.0
    } else {
        used / (span - left).max(span / 10.0)
    };
    Some(used + per_second * left * share)
}

/// The pace of one window at `now`: its use, run on at its present rate
/// ([`at_reset`]), against what is left before it resets.
fn window_pace(w: &WindowStatus, now: u64) -> Pace {
    if w.used_percent.is_some_and(|used| used >= 100.0) {
        return Pace::UsedUp;
    }
    let (Some(at_reset), Some(resets_at), Some(minutes)) =
        (at_reset(w, now, 1.0), w.resets_at, w.minutes)
    else {
        return Pace::Steady;
    };
    let left = resets_at.saturating_sub(now) as f64;
    if at_reset >= 100.0 {
        Pace::Tight
    } else if left <= (minutes * 60) as f64 / 4.0 && 100.0 - at_reset >= SPARE_PERCENT {
        Pace::Spare
    } else {
        Pace::Steady
    }
}

/// How much of window `w` is used at its reset were one more session to
/// start on its account at `now`, `running` already on it, where that is 100
/// or more: it runs out before it resets. The sessions running share its
/// present rate ([`at_reset`]), so one more adds one share of it; with none
/// running, the new one runs at the whole rate. None where it lasts, or its
/// use, reset or length is not known.
pub fn runs_out(w: &WindowStatus, running: u32, now: u64) -> Option<f64> {
    let share = (f64::from(running) + 1.0) / f64::from(running.max(1));
    at_reset(w, now, share).filter(|at| *at >= 100.0)
}

/// When the account of `status` may be read next: none when it may be now.
/// It waits while its latest reading in force (a window's, or a balance's)
/// is [`READ_AGAIN`] old or fresher, or while its last try, whatever came
/// of it, is under [`READ_AGAIN`] old. A refused one waits for a login
/// ([`Status::refused`]), which this does not tell.
pub fn read_next(status: &Status, now: u64) -> Option<u64> {
    let in_force = status
        .windows
        .iter()
        .filter(|w| w.source == Source::Read && w.used_percent.is_some())
        .map(|w| w.at)
        .chain(status.balances.iter().map(|b| b.at))
        .max();
    let after_reading = in_force.map(|at| at + READ_AGAIN + 1);
    let after_try = status.tried_at.map(|at| at + READ_AGAIN);
    [after_reading, after_try]
        .into_iter()
        .flatten()
        .max()
        .filter(|next| *next > now)
}

/// Whether a reading of `windows` says what the books, `entries` oldest
/// first, do not: unless they end on a reading of these very windows. A
/// caller that reads often (Claude Code's status line) books only news.
pub fn is_news(entries: &[Entry], windows: &[Window]) -> bool {
    !matches!(entries.last(), Some(Entry { what: What::Read(last), .. }) if last == windows)
}

#[cfg(test)]
#[path = "books_tests.rs"]
mod tests;
