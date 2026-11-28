//! The pick's rules, as Guild's ledger tests hold them
//! (`crates/ledger/tests/{quota,promises}.rs`), carried over to the
//! accounts of one engine.
use super::*;
use crate::books::{Entry, What, Window, status};

const NOW: u64 = 1_800_000_000;
const MINUTE: u64 = 60;

/// claude-code's windows, as its reader turns them out.
fn window(name: &str, used: f64, resets_at: u64) -> Window {
    let minutes = if name == "five_hour" {
        300
    } else {
        7 * 24 * 60
    };
    Window {
        name: name.into(),
        group: None,
        models: Vec::new(),
        used_percent: used,
        resets_at: Some(resets_at),
        minutes: Some(minutes),
        read_by: None,
    }
}

/// `windows`, each `(name, used, resets_at)`, read at `at`.
fn said(at: u64, windows: &[(&str, f64, u64)]) -> Entry {
    Entry {
        at,
        what: What::Read(
            windows
                .iter()
                .map(|&(name, used, resets_at)| window(name, used, resets_at))
                .collect(),
        ),
    }
}

/// A five-hour window read a minute ago, `used` percent used, resetting in
/// `minutes`.
fn five_hour(used: f64, minutes: u64) -> Vec<Entry> {
    vec![said(
        NOW - MINUTE,
        &[("five_hour", used, NOW + minutes * MINUTE)],
    )]
}

/// A week read three hours ago, `used` percent used, resetting in three
/// days: too old to run on at any rate, so only its use counts.
fn steady(used: f64) -> Vec<Entry> {
    vec![said(
        NOW - 180 * MINUTE,
        &[("seven_day", used, NOW + 3 * 1440 * MINUTE)],
    )]
}

/// The account `name`, its books `entries` at `now`, none running.
fn acct(name: &'static str, entries: &[Entry], now: u64) -> Candidate<'static> {
    Candidate {
        name,
        barred: None,
        running: 0,
        status: status(entries, now),
    }
}

/// Why the one account of `entries`, `running` on it, is passed over at
/// `now`: its reason and until when; none where it is picked.
fn passed_over(entries: &[Entry], running: u32, now: u64) -> Option<(Reason, Option<u64>)> {
    let mut a = acct("solo", entries, now);
    a.running = running;
    match pick(&[a], STOP_PERCENT, now) {
        Pick::Account(_) => None,
        Pick::None { why, until } => {
            assert_eq!(until, why[0].until);
            Some((why[0].reason, why[0].until))
        }
    }
}

fn why_of(entries: &[Entry], running: u32) -> String {
    let mut a = acct("solo", entries, NOW);
    a.running = running;
    match pick(&[a], STOP_PERCENT, NOW) {
        Pick::None { why, .. } => why[0].why.clone(),
        picked => panic!("picked: {picked:?}"),
    }
}

// One more session against a window's rate (Guild's `Funds::runs_out`).

#[test]
fn a_window_allows_another_session_while_its_rate_with_it_lasts_to_the_reset() {
    // 20% used in two of five hours: 50% at reset at the present rate, but
    // under half used it runs on at no rate. One running, one more: none.
    assert_eq!(passed_over(&five_hour(20.0, 180), 1, NOW), None);
    assert_eq!(passed_over(&five_hour(20.0, 180), 3, NOW), None);
}

#[test]
fn a_window_that_one_more_session_would_run_out_names_its_numbers() {
    // 50% used in two of five hours: 125% at reset; one more of one running
    // doubles the rest, 200%.
    let resets = NOW + 180 * MINUTE;
    assert_eq!(
        passed_over(&five_hour(50.0, 180), 1, NOW),
        Some((Reason::RunsOut, Some(resets)))
    );
    let why = why_of(&five_hour(50.0, 180), 1);
    assert!(
        why.contains("five_hour is 50% used") && why.contains("1 running") && why.contains("200%"),
        "{why}"
    );
    // Under half used it runs on at no rate: nothing runs out of it.
    assert_eq!(passed_over(&five_hour(30.0, 180), 1, NOW), None);
}

#[test]
fn the_sessions_running_share_the_rate_one_more_adds_one_share() {
    // 50% used in two of five hours, three running: 125%, one more adds a
    // third of the rest, 150%.
    assert!(why_of(&five_hour(50.0, 180), 3).contains("150%"));
    // 54% two hours from reset, three hours open: 90% at its rate; three
    // running, one more adds a third of the rest, 102%. Under half used,
    // none.
    assert!(passed_over(&five_hour(54.0, 120), 3, NOW).is_some());
    assert_eq!(passed_over(&five_hour(40.0, 120), 3, NOW), None);
}

#[test]
fn with_none_running_a_window_allows_one_while_its_present_rate_lasts() {
    // The first session runs at the window's whole rate: 75% at reset.
    assert_eq!(passed_over(&five_hour(30.0, 180), 0, NOW), None);
    assert_eq!(
        passed_over(&five_hour(50.0, 180), 0, NOW).map(|p| p.0),
        Some(Reason::RunsOut)
    );
}

#[test]
fn an_account_whose_use_is_not_known_is_picked() {
    assert_eq!(passed_over(&[], 5, NOW), None);
}

#[test]
fn of_several_windows_that_run_out_the_soonest_to_reset_is_said() {
    let five = NOW + 60 * MINUTE;
    let week = NOW + 140 * 60 * MINUTE;
    let book = [said(
        NOW - MINUTE,
        &[("seven_day", 54.0, week), ("five_hour", 85.0, five)],
    )];
    assert_eq!(
        passed_over(&book, 0, NOW),
        Some((Reason::RunsOut, Some(five)))
    );
}

// A limit (Guild's `a_usage_limit_that_holds_is_spent_until_it_lifts`).

fn limit(at: u64, window: Option<&str>, resets_at: Option<u64>) -> Entry {
    Entry {
        at,
        what: What::Limit {
            window: window.map(Into::into),
            resets_at,
        },
    }
}

#[test]
fn a_limit_that_holds_takes_no_session_until_it_lifts() {
    let lifts = NOW + 100 * MINUTE;
    let mut book = five_hour(40.0, 100);
    book.push(limit(NOW - MINUTE, Some("five_hour"), Some(lifts)));
    assert_eq!(
        passed_over(&book, 0, NOW),
        Some((Reason::Limit, Some(lifts)))
    );
    assert!(why_of(&book, 0).contains("its five_hour's limit"));
    assert_eq!(passed_over(&book, 0, lifts), None);
}

#[test]
fn an_engine_that_reports_no_windows_rests_after_a_limit_until_the_reset_it_names() {
    let lifts = NOW + 30 * MINUTE;
    let book = [limit(NOW - MINUTE, None, Some(lifts))];
    assert_eq!(
        passed_over(&book, 0, NOW),
        Some((Reason::Limit, Some(lifts)))
    );
    assert!(why_of(&book, 0).contains("the account's limit"));
    assert_eq!(passed_over(&book, 0, NOW + 31 * MINUTE), None);
    // One with no reset lifts at the next reading: no time to wait to.
    let book = [limit(NOW - MINUTE, None, None)];
    assert_eq!(passed_over(&book, 0, NOW), Some((Reason::Limit, None)));
    assert!(why_of(&book, 0).contains("its next reading"));
}

// The stop (Guild's `Funds::stopped`, promise `stopped_account_starts_none`).

#[test]
fn an_account_ninety_percent_used_of_a_window_takes_no_new_session_until_it_resets() {
    let resets = NOW + 100 * MINUTE;
    let a = acct("claude-a", &five_hour(91.0, 100), NOW);
    let b = acct("claude-b", &five_hour(40.0, 100), NOW);
    assert_eq!(
        pick(
            &[a, acct("claude-b", &five_hour(40.0, 100), NOW)],
            STOP_PERCENT,
            NOW
        ),
        Pick::Account("claude-b".into())
    );
    let Pick::None { until, why } = pick(
        &[acct("claude-a", &five_hour(91.0, 100), NOW)],
        STOP_PERCENT,
        NOW,
    ) else {
        panic!("picked a stopped account");
    };
    assert_eq!(until, Some(resets));
    assert_eq!(
        (why[0].account.as_str(), why[0].reason),
        ("claude-a", Reason::Stopped)
    );
    assert!(
        why[0]
            .why
            .contains("five_hour is 91% used, at its stop of 90%"),
        "{}",
        why[0].why
    );
    assert_eq!(left_percent(&b.status), Some(60.0));

    // Once its window resets the reading no longer holds: it takes sessions.
    let later = NOW + 101 * MINUTE;
    assert_eq!(passed_over(&five_hour(91.0, 100), 0, later), None);
}

#[test]
fn the_stop_is_a_setting() {
    assert_eq!(STOP_PERCENT, 90);
    assert_eq!(
        pick(&[acct("claude-a", &steady(91.0), NOW)], 95, NOW),
        Pick::Account("claude-a".into())
    );
    // At the stop exactly, stopped.
    assert!(matches!(
        pick(&[acct("claude-a", &steady(95.0), NOW)], 95, NOW),
        Pick::None { .. }
    ));
}

#[test]
fn a_week_at_ninety_percent_is_stopped() {
    let resets = NOW + 78 * 60 * MINUTE;
    let book = [said(NOW - MINUTE, &[("seven_day", 90.0, resets)])];
    assert_eq!(
        passed_over(&book, 0, NOW),
        Some((Reason::Stopped, Some(resets)))
    );
}

// No count of sessions running passes an account over by itself: its
// windows and their pace decide (stablehand#26).

#[test]
fn any_number_running_is_judged_only_by_the_windows_and_their_pace() {
    let mut busy = acct("busy", &[], NOW);
    busy.running = 40;
    assert_eq!(
        pick(&[busy], STOP_PERCENT, NOW),
        Pick::Account("busy".into())
    );
}

// What the register and the login bar (Guild's `a_tight_account_switched_off_is_off`).

#[test]
fn a_barred_account_is_passed_over_whatever_its_books_say() {
    let mut off = acct("claude-a", &five_hour(10.0, 100), NOW);
    off.barred = Some((Reason::Off, "switched off: card declined".into()));
    let Pick::None { until, why } = pick(&[off], STOP_PERCENT, NOW) else {
        panic!("picked an account off");
    };
    assert_eq!(until, None);
    assert_eq!(
        (why[0].reason, why[0].why.as_str()),
        (Reason::Off, "switched off: card declined")
    );
}

// The order (Guild's `Funds::left_percent`: the most left first).

#[test]
fn of_those_that_may_take_one_the_most_left_is_picked_and_no_reading_last() {
    let accounts = [
        acct("never-read", &[], NOW),
        acct("half", &steady(45.0), NOW),
        acct("most", &steady(10.0), NOW),
        acct("little", &steady(80.0), NOW),
    ];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("most".into())
    );
    // Its most used window decides how much is left (their weeks resetting
    // together).
    let both = |five, week| {
        [said(
            NOW - 180 * MINUTE,
            &[
                ("five_hour", five, NOW + 280 * MINUTE),
                ("seven_day", week, NOW + 3 * 1440 * MINUTE),
            ],
        )]
    };
    let accounts = [
        acct("week-used", &both(5.0, 70.0), NOW),
        acct("half", &both(45.0, 20.0), NOW),
    ];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("half".into())
    );
    // An account never read comes after every one read, however little is left.
    let accounts = [
        acct("never-read", &[], NOW),
        acct("little", &steady(80.0), NOW),
    ];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("little".into())
    );
    // Of equals, the first given.
    let accounts = [acct("a", &[], NOW), acct("b", &[], NOW)];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("a".into())
    );
}

#[test]
fn an_account_read_only_stale_comes_after_every_one_read_fresh() {
    let mut stale = five_hour(5.0, 280);
    stale.push(Entry {
        at: NOW,
        what: What::Failed("HTTP 401".into()),
    });
    let accounts = [
        acct("stale", &stale, NOW),
        acct("little", &steady(80.0), NOW),
    ];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("little".into())
    );
}

#[test]
fn none_waits_until_the_earliest_any_lifts() {
    let soon = NOW + 30 * MINUTE;
    let mut off = acct("off", &[], NOW);
    off.barred = Some((Reason::Off, "switched off".into()));
    let accounts = [
        acct("stopped", &five_hour(95.0, 100), NOW),
        off,
        acct("limited", &[limit(NOW - MINUTE, None, Some(soon))], NOW),
    ];
    let Pick::None { until, why } = pick(&accounts, STOP_PERCENT, NOW) else {
        panic!("picked");
    };
    assert_eq!(until, Some(soon));
    let reasons: Vec<_> = why.iter().map(|p| (p.account.as_str(), p.reason)).collect();
    assert_eq!(
        reasons,
        [
            ("stopped", Reason::Stopped),
            ("off", Reason::Off),
            ("limited", Reason::Limit)
        ]
    );
    assert_eq!(
        pick(&[], STOP_PERCENT, NOW),
        Pick::None {
            until: None,
            why: vec![]
        }
    );
}

// The rate, from the readings (Guild's quota.rs, its book of control#1295).

#[test]
fn a_window_climbing_fast_across_two_readings_runs_out() {
    let resets = NOW + 140 * 60 * MINUTE;
    let book = [
        said(NOW - 130 * MINUTE, &[("seven_day", 54.0, resets)]),
        said(NOW - 10 * MINUTE, &[("seven_day", 64.0, resets)]),
    ];
    assert_eq!(
        passed_over(&book, 0, NOW),
        Some((Reason::RunsOut, Some(resets)))
    );
}

#[test]
fn a_single_reading_is_paced_by_the_use_since_its_window_opened() {
    // 54% used 25 hours into the week: 359% at reset.
    let resets = NOW + 143 * 60 * MINUTE;
    let book = [said(NOW - 10 * MINUTE, &[("seven_day", 54.0, resets)])];
    assert_eq!(
        passed_over(&book, 0, NOW).map(|p| p.0),
        Some(Reason::RunsOut)
    );
}

#[test]
fn a_reading_older_than_two_hours_runs_out_of_nothing_on_its_own() {
    let resets = NOW + 140 * 60 * MINUTE;
    let book = [said(NOW - 3 * 60 * MINUTE, &[("seven_day", 64.0, resets)])];
    assert_eq!(passed_over(&book, 0, NOW), None);
}

#[test]
fn a_night_s_burst_in_a_week_under_half_used_runs_out_of_nothing() {
    // 15% to 24% in 7h, 78h left of a week: 108% at reset by the burst run
    // on, yet only 24% used (sunfmin/control#1358).
    let resets = NOW + 78 * 60 * MINUTE;
    let book = [
        said(NOW - 420 * MINUTE, &[("seven_day", 15.0, resets)]),
        said(NOW - MINUTE, &[("seven_day", 24.0, resets)]),
    ];
    assert_eq!(passed_over(&book, 0, NOW), None);
}

#[test]
fn a_window_flat_across_its_readings_runs_out_of_nothing_however_early_its_use_came() {
    // control#1295: seven_day at 34%, 25 hours into its week, flat.
    const WEEK_RESETS: u64 = 1_795_345_200;
    let book = [
        said(1_794_829_689, &[("seven_day", 34.0, WEEK_RESETS)]),
        said(1_794_830_114, &[("seven_day", 34.0, WEEK_RESETS)]),
        said(1_794_830_770, &[("seven_day", 34.0, WEEK_RESETS)]),
    ];
    assert_eq!(passed_over(&book, 0, 1_794_830_770 + 10 * MINUTE), None);
}

// A session of a model, of an account whose windows are grouped (agy's).

/// agy's books, read a minute ago: its Gemini group's 5h window `gemini`
/// percent used, its Claude-and-GPT group's `third` percent; each resetting
/// in three hours; and a window in no group, `other` percent used.
fn agy(gemini: f64, third: f64, other: Option<f64>) -> Status {
    let w = |name: &str, group: &str, models: &[&str], used| Window {
        name: name.into(),
        group: Some(group.into()),
        models: models.iter().map(|m| m.to_string()).collect(),
        used_percent: used,
        resets_at: Some(NOW + 180 * MINUTE),
        minutes: Some(300),
        read_by: None,
    };
    let mut windows = vec![
        w(
            "gemini-5h",
            "Gemini Models",
            &["gemini-4-neon-high"],
            gemini,
        ),
        w(
            "3p-5h",
            "Claude and GPT models",
            &["claude-opus-5-5-high"],
            third,
        ),
    ];
    if let Some(used) = other {
        windows.push(Window {
            group: None,
            models: Vec::new(),
            ..window("five_hour", used, NOW + 180 * MINUTE)
        });
    }
    status(
        &[Entry {
            at: NOW - MINUTE,
            what: What::Read(windows),
        }],
        NOW,
    )
}

fn agy_pick(s: Status, model: Option<&str>) -> Result<String, (Reason, String)> {
    let s = match model {
        Some(m) => spent(s, m).0,
        None => s,
    };
    let a = Candidate {
        name: "agy",
        barred: None,
        running: 0,
        status: s,
    };
    match pick(&[a], STOP_PERCENT, NOW) {
        Pick::Account(name) => Ok(name),
        Pick::None { why, .. } => Err((why[0].reason, why[0].why.clone())),
    }
}

#[test]
fn a_model_is_judged_by_its_group_s_windows_and_those_in_no_group() {
    // The Gemini group used up: a Claude session may start, a Gemini one not.
    assert_eq!(
        agy_pick(agy(100.0, 0.0, None), Some("claude-opus-5-5-high")),
        Ok("agy".into())
    );
    let (reason, why) = agy_pick(agy(100.0, 0.0, None), Some("gemini-4-neon-high")).unwrap_err();
    assert_eq!(reason, Reason::Stopped);
    assert!(why.contains("gemini-5h is 100% used"), "{why}");
    // Without a model, or of one no group holds, every window decides.
    for model in [None, Some("gpt-9")] {
        assert_eq!(
            agy_pick(agy(100.0, 0.0, None), model).unwrap_err().0,
            Reason::Stopped
        );
    }
    let in_no_group = |m| spent(agy(100.0, 0.0, None), m).1;
    assert!(in_no_group("gpt-9") && !in_no_group("claude-opus-5-5-high"));
    // A window in no group decides for every model.
    let (_, why) = agy_pick(agy(0.0, 0.0, Some(95.0)), Some("claude-opus-5-5-high")).unwrap_err();
    assert!(why.contains("five_hour is 95% used"), "{why}");
    // Of the group's windows, its most used decides how much is left.
    let left = |m| left_percent(&spent(agy(80.0, 30.0, None), m).0);
    assert_eq!(
        (left("claude-opus-5-5-high"), left("gemini-4-neon-high")),
        (Some(70.0), Some(20.0))
    );
}

#[test]
fn a_model_s_windows_carry_a_limit_on_them_and_not_one_on_another_group() {
    let limited = |window: Option<&str>| {
        let mut s = agy(0.0, 0.0, None);
        s.limit = Some(books::Held {
            at: NOW,
            window: window.map(Into::into),
            resets_at: None,
        });
        s
    };
    let claude = Some("claude-opus-5-5-high");
    assert_eq!(
        agy_pick(limited(Some("gemini-5h")), claude),
        Ok("agy".into())
    );
    for window in [Some("3p-5h"), None] {
        assert_eq!(
            agy_pick(limited(window), claude).unwrap_err().0,
            Reason::Limit
        );
    }
}

#[test]
fn ungrouped_windows_know_no_model_and_every_one_decides() {
    let s = status(&five_hour(95.0, 100), NOW);
    assert_eq!(spent(s.clone(), "claude-opus-5-5"), (s, false));
}

// Within a step, the quota that resets soonest is spent first: the earliest
// reset of each account's long window, then the most left.

const HOUR: u64 = 60 * MINUTE;

/// A claude-code account read a minute ago: its five_hour 10% used, its
/// seven_day `used` percent used, resetting in `hours` (none: not said).
fn week_left(name: &'static str, used: f64, hours: Option<u64>) -> Candidate<'static> {
    let mut week = window("seven_day", used, 0);
    week.resets_at = hours.map(|h| NOW + h * HOUR);
    let windows = vec![window("five_hour", 10.0, NOW + 4 * HOUR), week];
    let entries = [Entry {
        at: NOW - MINUTE,
        what: What::Read(windows),
    }];
    acct(name, &entries, NOW)
}

#[test]
fn the_account_whose_long_window_resets_first_is_picked_until_it_stops() {
    // Felix's claude-code / opus step, 2026-11-28.
    let today = |code_2: f64, nate: f64| {
        [
            week_left("claude-theplant-jp", 16.0, Some(6 * 24 + 6)),
            week_left("claude-nate-ultra", nate, Some(2 * 24 + 12)),
            week_left("claude-code-2", code_2, Some(24 + 2)),
        ]
    };
    let picked = |accounts: &[Candidate]| pick(accounts, STOP_PERCENT, NOW);
    let account = |name: &str| Pick::Account(name.into());
    assert_eq!(picked(&today(85.0, 75.0)), account("claude-code-2"));
    assert_eq!(picked(&today(90.0, 75.0)), account("claude-nate-ultra"));
    assert_eq!(picked(&today(90.0, 90.0)), account("claude-theplant-jp"));
}

#[test]
fn an_account_whose_long_window_s_reset_is_not_known_comes_after_one_known() {
    let accounts = [
        week_left("unknown", 10.0, None),
        week_left("known", 70.0, Some(5 * 24)),
    ];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("known".into())
    );
    // Given the other way round, the known reset still goes first.
    let accounts = [
        week_left("known", 70.0, Some(5 * 24)),
        week_left("unknown", 10.0, None),
    ];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("known".into())
    );
    // A reset come already tells nothing of the next one: of equals left,
    // the first given.
    let accounts = [
        week_left("unknown", 10.0, None),
        week_left("reset", 10.0, Some(0)),
    ];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("unknown".into())
    );
    // Of equals in both, the first given.
    let accounts = [
        week_left("first", 40.0, Some(5 * 24)),
        week_left("second", 40.0, Some(5 * 24)),
    ];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("first".into())
    );
    // Of equal resets, and of those not known, the most left.
    let accounts = [
        week_left("less", 70.0, Some(5 * 24)),
        week_left("more", 40.0, Some(5 * 24)),
    ];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("more".into())
    );
    let accounts = [week_left("less", 70.0, None), week_left("more", 40.0, None)];
    assert_eq!(
        pick(&accounts, STOP_PERCENT, NOW),
        Pick::Account("more".into())
    );
}

#[test]
fn a_long_window_that_runs_out_before_its_reset_passes_over_nothing() {
    // 85% of the week used, 26 hours from its reset: 101% at its rate.
    let a = week_left("claude-code-2", 85.0, Some(26));
    let week = a.status.windows.iter().find(|w| w.name == "seven_day");
    assert!(books::runs_out(week.unwrap(), 0, NOW).is_some());
    assert_eq!(
        pick(&[a], STOP_PERCENT, NOW),
        Pick::Account("claude-code-2".into())
    );
    // Its five_hour still guards a session cut off mid-work.
    let book = [said(
        NOW - MINUTE,
        &[
            ("five_hour", 60.0, NOW + 3 * HOUR),
            ("seven_day", 20.0, NOW + 100 * HOUR),
        ],
    )];
    assert_eq!(
        passed_over(&book, 0, NOW),
        Some((Reason::RunsOut, Some(NOW + 3 * HOUR)))
    );
}
