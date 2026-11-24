//! The books' rules, as Guild's ledger tests hold them
//! (`crates/ledger/tests/{books,quota,read_again}.rs`), carried over to
//! one account's entries.
use super::*;

const NOW: u64 = 1_800_000_000;
const MINUTE: u64 = 60;
const HOUR: u64 = 3600;

/// claude-code's windows, as its reader turns them out.
fn window(name: &str, used: f64, resets_at: Option<u64>) -> Window {
    let minutes = if name == "five_hour" {
        300
    } else {
        7 * 24 * 60
    };
    Window {
        name: name.into(),
        group: None,
        used_percent: used,
        resets_at,
        minutes: Some(minutes),
    }
}

fn read(at: u64, windows: Vec<Window>) -> Entry {
    Entry {
        at,
        what: What::Read(windows),
    }
}

fn failed(at: u64) -> Entry {
    Entry {
        at,
        what: What::Failed("HTTP 401".into()),
    }
}

fn limit(at: u64, window: Option<&str>, resets_at: Option<u64>) -> Entry {
    Entry {
        at,
        what: What::Limit {
            window: window.map(Into::into),
            resets_at,
        },
    }
}

fn named<'a>(s: &'a Status, name: &str) -> &'a WindowStatus {
    s.windows
        .iter()
        .find(|w| w.name == name)
        .unwrap_or_else(|| panic!("no window {name} in {s:?}"))
}

#[test]
fn a_read_window_is_the_engine_s_percentage_and_reset() {
    let s = status(
        &[read(
            NOW - 60,
            vec![
                window("five_hour", 6.0, Some(NOW + 2 * HOUR)),
                window("seven_day", 19.0, Some(NOW + 50 * HOUR)),
            ],
        )],
        NOW,
    );
    let shown: Vec<_> = s
        .windows
        .iter()
        .map(|w| {
            (
                w.name.as_str(),
                w.used_percent,
                w.resets_at,
                w.source,
                w.at,
                w.stale,
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (
                "five_hour",
                Some(6.0),
                Some(NOW + 2 * HOUR),
                Source::Read,
                NOW - 60,
                false
            ),
            (
                "seven_day",
                Some(19.0),
                Some(NOW + 50 * HOUR),
                Source::Read,
                NOW - 60,
                false
            ),
        ]
    );
    assert_eq!(
        (s.read_at, s.tried_at, s.failed),
        (Some(NOW - 60), Some(NOW - 60), None)
    );
}

#[test]
fn the_latest_reading_stands() {
    let s = status(
        &[
            read(NOW - 60, vec![window("five_hour", 41.0, Some(NOW + HOUR))]),
            read(NOW - 600, vec![window("five_hour", 6.0, Some(NOW + HOUR))]),
        ],
        NOW,
    );
    assert_eq!(named(&s, "five_hour").used_percent, Some(41.0));
}

#[test]
fn a_failed_try_keeps_the_last_windows_and_marks_them_stale() {
    let s = status(
        &[
            read(
                NOW - 2 * HOUR,
                vec![window("five_hour", 41.0, Some(NOW + HOUR))],
            ),
            failed(NOW - HOUR),
        ],
        NOW,
    );
    let w = named(&s, "five_hour");
    assert_eq!((w.used_percent, w.stale), (Some(41.0), true));
    assert_eq!(s.failed.as_deref(), Some("HTTP 401"));
    assert_eq!(
        (s.read_at, s.tried_at),
        (Some(NOW - 2 * HOUR), Some(NOW - HOUR))
    );
    // A later reading clears it.
    let s = status(
        &[
            read(
                NOW - 2 * HOUR,
                vec![window("five_hour", 41.0, Some(NOW + HOUR))],
            ),
            failed(NOW - HOUR),
            read(NOW - 60, vec![window("five_hour", 45.0, Some(NOW + HOUR))]),
        ],
        NOW,
    );
    assert_eq!((named(&s, "five_hour").stale, s.failed), (false, None));
}

#[test]
fn a_reading_past_its_reset_has_no_use_known() {
    let s = status(
        &[read(
            NOW - 4 * HOUR,
            vec![window("five_hour", 95.0, Some(NOW - 2 * HOUR))],
        )],
        NOW,
    );
    let w = named(&s, "five_hour");
    assert_eq!(
        (w.used_percent, w.resets_at, w.stale, w.pace),
        (None, None, true, Pace::Steady)
    );
    // With no reset said, it holds for the window's length.
    let mut no_reset = window("five_hour", 95.0, None);
    let held = status(&[read(NOW - 4 * HOUR, vec![no_reset.clone()])], NOW);
    assert_eq!(named(&held, "five_hour").used_percent, Some(95.0));
    let gone = status(&[read(NOW - 6 * HOUR, vec![no_reset.clone()])], NOW);
    assert_eq!(named(&gone, "five_hour").used_percent, None);
    // Of no length either, it holds.
    no_reset.minutes = None;
    let kept = status(&[read(NOW - 600 * HOUR, vec![no_reset])], NOW);
    assert_eq!(named(&kept, "five_hour").used_percent, Some(95.0));
}

#[test]
fn a_limit_uses_up_its_window_until_it_resets() {
    let entries = [
        read(
            NOW - 600,
            vec![window("seven_day", 97.0, Some(NOW + 3 * HOUR))],
        ),
        limit(NOW - 60, Some("seven_day"), Some(NOW + 3 * HOUR)),
    ];
    let s = status(&entries, NOW);
    assert_eq!(
        s.limit,
        Some(Held {
            at: NOW - 60,
            window: Some("seven_day".into()),
            resets_at: Some(NOW + 3 * HOUR)
        })
    );
    let w = named(&s, "seven_day");
    assert_eq!(
        (w.used_percent, w.resets_at, w.source, w.at, w.pace),
        (
            Some(100.0),
            Some(NOW + 3 * HOUR),
            Source::Limit,
            NOW - 60,
            Pace::UsedUp
        )
    );
    assert_eq!(s.pace, Pace::UsedUp);
    // Once it resets, it no longer holds.
    let after = status(&entries, NOW + 4 * HOUR);
    assert_eq!(after.limit, None);
}

#[test]
fn a_limit_takes_its_reset_from_the_reading_where_it_names_none() {
    let s = status(
        &[
            read(
                NOW - 600,
                vec![window("seven_day", 97.0, Some(NOW + 3 * HOUR))],
            ),
            limit(NOW - 60, Some("seven_day"), None),
        ],
        NOW,
    );
    assert_eq!(named(&s, "seven_day").resets_at, Some(NOW + 3 * HOUR));
}

#[test]
fn a_reading_after_a_limit_stands_over_it_for_its_window() {
    let s = status(
        &[
            limit(NOW - 600, Some("five_hour"), Some(NOW + HOUR)),
            read(NOW - 60, vec![window("five_hour", 12.0, Some(NOW + HOUR))]),
        ],
        NOW,
    );
    assert_eq!(named(&s, "five_hour").source, Source::Read);
    // The account's limit still holds until its reset.
    assert_eq!(s.pace, Pace::UsedUp);
}

#[test]
fn a_limit_without_its_reset_holds_until_a_later_reading() {
    let mut entries = vec![limit(NOW - 600, None, None)];
    let s = status(&entries, NOW);
    assert_eq!(s.limit.map(|l| l.at), Some(NOW - 600));
    assert_eq!(s.pace, Pace::UsedUp);
    // A failed try does not lift it; a reading does.
    entries.push(failed(NOW - 300));
    assert!(status(&entries, NOW).limit.is_some());
    entries.push(read(
        NOW - 60,
        vec![window("five_hour", 2.0, Some(NOW + HOUR))],
    ));
    assert_eq!(status(&entries, NOW).limit, None);
}

#[test]
fn a_limit_on_an_account_never_read_is_its_only_window() {
    // A `claude setup-token` account: no reading, only the caller's limit.
    let s = status(
        &[limit(NOW - 60, Some("five_hour"), Some(NOW + 30 * MINUTE))],
        NOW,
    );
    let w = named(&s, "five_hour");
    assert_eq!(
        (w.used_percent, w.resets_at, w.source, w.pace),
        (
            Some(100.0),
            Some(NOW + 30 * MINUTE),
            Source::Limit,
            Pace::UsedUp
        )
    );
    assert_eq!((s.read_at, s.tried_at), (None, None));
}

#[test]
fn an_engine_with_no_windows_rests_after_a_limit_until_the_reset_it_names() {
    let entries = [limit(NOW - MINUTE, None, Some(NOW + 30 * MINUTE))];
    let s = status(&entries, NOW);
    assert!(s.windows.is_empty());
    assert_eq!(s.pace, Pace::UsedUp);
    assert_eq!(status(&entries, NOW + 31 * MINUTE).pace, Pace::Steady);
}

#[test]
fn an_entry_booked_later_than_now_is_not_yet_in_the_books() {
    let s = status(
        &[read(
            NOW + 60,
            vec![window("five_hour", 6.0, Some(NOW + HOUR))],
        )],
        NOW,
    );
    assert!(s.windows.is_empty());
    assert_eq!(s.read_at, None);
}

// Pace (Guild's quota.rs).

/// The pace of one five-hour window read a minute ago, `used` percent used
/// and resetting in `minutes`.
fn five_hour(used: f64, minutes: u64) -> Pace {
    status(
        &[read(
            NOW - MINUTE,
            vec![window("five_hour", used, Some(NOW + minutes * MINUTE))],
        )],
        NOW,
    )
    .pace
}

#[test]
fn ten_percent_left_forty_minutes_from_reset_is_tight() {
    assert_eq!(five_hour(90.0, 40), Pace::Tight);
}

#[test]
fn half_left_forty_minutes_from_reset_is_spare() {
    assert_eq!(five_hour(50.0, 40), Pace::Spare);
}

#[test]
fn a_window_far_from_reset_at_an_even_rate_is_steady() {
    // 30% used, three of five hours to go: 75% at reset.
    assert_eq!(five_hour(30.0, 180), Pace::Steady);
    // 50% in two of five hours: 125% at reset.
    assert_eq!(five_hour(50.0, 180), Pace::Tight);
}

#[test]
fn a_window_used_up_is_used_up() {
    assert_eq!(five_hour(100.0, 180), Pace::UsedUp);
}

#[test]
fn the_account_s_pace_is_its_worst_window_s() {
    let s = status(
        &[read(
            NOW - MINUTE,
            vec![
                window("five_hour", 90.0, Some(NOW + 40 * MINUTE)),
                window("seven_day", 50.0, Some(NOW + 40 * MINUTE)),
            ],
        )],
        NOW,
    );
    assert_eq!(named(&s, "five_hour").pace, Pace::Tight);
    assert_eq!(named(&s, "seven_day").pace, Pace::Spare);
    assert_eq!(s.pace, Pace::Tight);
}

/// `windows`, each `(name, used, resets_at)`, read at `at`.
fn said(at: u64, windows: &[(&str, f64, u64)]) -> Entry {
    read(
        at,
        windows
            .iter()
            .map(|&(name, used, resets_at)| window(name, used, Some(resets_at)))
            .collect(),
    )
}

fn week_pace(book: &[Entry], now: u64) -> Pace {
    named(&status(book, now), "seven_day").pace
}

#[test]
fn a_window_flat_across_its_readings_is_steady_however_early_its_use_came() {
    // sunfmin/control#1295: four runs in 18 minutes left seven_day at 34%,
    // 25 hours into its week, and nothing since.
    const WEEK_RESETS: u64 = 1_795_345_200;
    const FIVE_RESETS: u64 = 1_794_834_000;
    const FIVE_AGAIN: u64 = 1_794_848_728;
    let book = [
        said(
            1_794_829_689,
            &[
                ("five_hour", 35.0, FIVE_RESETS),
                ("seven_day", 34.0, WEEK_RESETS),
            ],
        ),
        said(1_794_829_809, &[("seven_day", 34.0, WEEK_RESETS)]),
        said(
            1_794_830_114,
            &[
                ("five_hour", 36.0, FIVE_RESETS),
                ("seven_day", 34.0, WEEK_RESETS),
            ],
        ),
        said(
            1_794_830_728,
            &[
                ("five_hour", 1.0, FIVE_AGAIN),
                ("seven_day", 34.0, WEEK_RESETS),
            ],
        ),
        said(1_794_830_770, &[("seven_day", 34.0, WEEK_RESETS)]),
    ];
    assert_eq!(week_pace(&book, 1_794_830_770 + 10 * MINUTE), Pace::Steady);
    assert_eq!(week_pace(&book, 1_794_875_491), Pace::Steady);
}

#[test]
fn a_window_climbing_fast_across_two_readings_is_tight() {
    let resets = NOW + 140 * 60 * MINUTE;
    let book = [
        said(NOW - 130 * MINUTE, &[("seven_day", 54.0, resets)]),
        said(NOW - 10 * MINUTE, &[("seven_day", 64.0, resets)]),
    ];
    assert_eq!(week_pace(&book, NOW), Pace::Tight);
    assert!(named(&status(&book, NOW), "seven_day").rate.is_some());
}

#[test]
fn a_single_reading_is_paced_by_the_use_since_its_window_opened() {
    // 54% used 25 hours into the week: 359% at reset.
    let resets = NOW + 143 * 60 * MINUTE;
    let book = [said(NOW - 10 * MINUTE, &[("seven_day", 54.0, resets)])];
    assert_eq!(week_pace(&book, NOW), Pace::Tight);
}

#[test]
fn a_reading_older_than_two_hours_makes_no_window_tight_on_its_own() {
    let resets = NOW + 140 * 60 * MINUTE;
    let book = [said(NOW - 3 * 60 * MINUTE, &[("seven_day", 54.0, resets)])];
    assert_eq!(week_pace(&book, NOW), Pace::Steady);
}

#[test]
fn a_night_s_burst_in_a_week_under_half_used_is_not_tight() {
    // 15% to 24% in 7h, 78h left of a week: 108% at reset by the burst run
    // on, yet only 24% used (sunfmin/control#1358).
    let resets = NOW + 78 * 60 * MINUTE;
    let book = [
        said(NOW - 420 * MINUTE, &[("seven_day", 15.0, resets)]),
        said(NOW - MINUTE, &[("seven_day", 24.0, resets)]),
    ];
    assert_eq!(week_pace(&book, NOW), Pace::Steady);
}

// Reading again (Guild's read_again.rs).

/// When an account whose seven-day window was read `ago` seconds before
/// now, and tried as `tried` says, may be read next.
fn next(ago: Option<u64>, tried: Option<u64>) -> Option<u64> {
    let mut book = Vec::new();
    if let Some(ago) = ago {
        book.push(said(NOW - ago, &[("seven_day", 33.0, NOW + 100 * HOUR)]));
    }
    if let Some(ago) = tried {
        book.push(failed(NOW - ago));
    }
    read_next(&status(&book, NOW), NOW)
}

#[test]
fn an_account_whose_reading_is_over_an_hour_old_is_read_again() {
    assert_eq!(next(Some(READ_AGAIN + MINUTE), None), None);
}

#[test]
fn a_reading_an_hour_old_or_fresher_is_not_read_again() {
    assert_eq!(next(Some(READ_AGAIN), None), Some(NOW + 1));
    assert_eq!(next(Some(10 * MINUTE), None), Some(NOW + 50 * MINUTE + 1));
}

#[test]
fn an_account_tried_in_the_last_hour_is_not_tried_again_whatever_came_of_it() {
    assert_eq!(
        next(Some(13 * HOUR), Some(10 * MINUTE)),
        Some(NOW + 50 * MINUTE)
    );
    assert_eq!(next(Some(13 * HOUR), Some(READ_AGAIN)), None);
}

#[test]
fn an_account_with_no_reading_in_force_is_read() {
    assert_eq!(next(None, None), None);
    // One read before its window reset is none in force.
    let book = [said(
        NOW - 30 * MINUTE,
        &[("five_hour", 33.0, NOW - MINUTE)],
    )];
    let s = status(&book, NOW);
    assert_eq!(read_next(&s, NOW), Some(NOW + 30 * MINUTE));
}

#[test]
fn the_recent_rate_runs_from_the_latest_reading_a_twentieth_of_the_window_back() {
    // Five hours: a twentieth is 15 minutes. Of the readings 3600 s and
    // 1200 s before the latest, the later one is the baseline.
    let resets = NOW + 2 * HOUR;
    let book = [
        said(NOW - 3600 - 60, &[("five_hour", 50.0, resets)]),
        said(NOW - 1200 - 60, &[("five_hour", 60.0, resets)]),
        said(NOW - 60, &[("five_hour", 70.0, resets)]),
    ];
    let rate = named(&status(&book, NOW), "five_hour").rate.unwrap();
    assert!((rate - 10.0 * 3600.0 / 1200.0).abs() < 1e-9, "{rate}");
}

#[test]
fn a_window_near_its_reset_with_little_left_at_its_rate_is_steady_not_spare() {
    // 85% used, 40 minutes left of five hours: 98% at reset.
    assert_eq!(five_hour(85.0, 40), Pace::Steady);
}

#[test]
fn a_reading_exactly_two_hours_old_still_paces_its_window() {
    let resets = NOW + 143 * 60 * MINUTE;
    let book = [said(NOW - REPORT_FRESH, &[("seven_day", 54.0, resets)])];
    assert_eq!(week_pace(&book, NOW), Pace::Tight);
}

#[test]
fn a_limit_lifts_at_the_second_it_resets() {
    let entries = [limit(NOW - 60, Some("five_hour"), Some(NOW + HOUR))];
    assert!(status(&entries, NOW + HOUR - 1).limit.is_some());
    assert_eq!(status(&entries, NOW + HOUR).limit, None);
}

#[test]
fn a_limit_takes_no_reset_from_a_reading_whose_reset_is_now() {
    let entries = [
        read(
            NOW - 600,
            vec![window("seven_day", 97.0, Some(NOW + 3 * HOUR))],
        ),
        limit(NOW - 60, Some("seven_day"), None),
    ];
    let w = status(&entries, NOW + 3 * HOUR).windows[0].clone();
    assert_eq!((w.source, w.resets_at), (Source::Limit, None));
}

fn wallets(at: u64, normal: f64) -> Entry {
    Entry {
        at,
        what: What::Balance(vec![
            Balance {
                wallet: "normal".into(),
                currency: "CNY".into(),
                amount: normal,
            },
            Balance {
                wallet: "bonus".into(),
                currency: "CNY".into(),
                amount: 0.0,
            },
        ]),
    }
}

fn refused(at: u64) -> Entry {
    Entry {
        at,
        what: What::Refused("code 40003".into()),
    }
}

#[test]
fn a_balance_reading_is_its_latest_wallets_with_no_window() {
    let s = status(
        &[wallets(NOW - 2 * HOUR, 99.0), wallets(NOW - HOUR, 94.67)],
        NOW,
    );
    let shown: Vec<_> = s
        .balances
        .iter()
        .map(|b| {
            (
                b.balance.wallet.as_str(),
                b.balance.currency.as_str(),
                b.balance.amount,
                b.at,
                b.stale,
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("normal", "CNY", 94.67, NOW - HOUR, false),
            ("bonus", "CNY", 0.0, NOW - HOUR, false),
        ]
    );
    assert!(s.windows.is_empty());
    assert_eq!(
        (s.read_at, s.tried_at),
        (Some(NOW - HOUR), Some(NOW - HOUR))
    );
    assert_eq!(s.pace, Pace::Steady);
    // It is read again after the hour, as a window reading is.
    assert_eq!(read_next(&s, NOW), Some(NOW + 1));
    assert_eq!(read_next(&s, NOW + 2), None);
}

#[test]
fn a_failed_try_keeps_the_last_balance_and_marks_it_stale() {
    let s = status(&[wallets(NOW - 2 * HOUR, 94.67), failed(NOW - HOUR)], NOW);
    assert_eq!(
        (s.balances[0].balance.amount, s.balances[0].stale),
        (94.67, true)
    );
    assert_eq!(s.failed.as_deref(), Some("HTTP 401"));
    assert!(!s.refused);
}

#[test]
fn a_refused_credential_is_held_until_a_login_after_it() {
    let s = status(
        &[wallets(NOW - 3 * HOUR, 94.67), refused(NOW - 2 * HOUR)],
        NOW,
    );
    assert!(s.refused);
    assert_eq!(s.failed.as_deref(), Some("code 40003"));
    assert_eq!(
        (s.balances[0].balance.amount, s.balances[0].stale),
        (94.67, true)
    );
    assert_eq!(s.tried_at, Some(NOW - 2 * HOUR));
    let login = Entry {
        at: NOW - HOUR,
        what: What::LoggedIn,
    };
    let s = status(
        &[
            wallets(NOW - 3 * HOUR, 94.67),
            refused(NOW - 2 * HOUR),
            login,
        ],
        NOW,
    );
    assert!(!s.refused);
    // A login is no try: the refusal still says why the last one failed.
    assert_eq!(s.tried_at, Some(NOW - 2 * HOUR));
    assert_eq!(read_next(&s, NOW), None);
}

#[test]
fn a_reading_is_news_unless_the_books_end_on_the_same_one() {
    let windows = vec![
        window("five_hour", 23.5, Some(NOW + HOUR)),
        window("seven_day", 41.0, Some(NOW + 3 * 24 * HOUR)),
    ];
    let mut more = windows.clone();
    more[0].used_percent = 24.0;
    let same = read(NOW - MINUTE, windows.clone());
    assert!(is_news(&[], &windows));
    assert!(!is_news(std::slice::from_ref(&same), &windows));
    assert!(is_news(std::slice::from_ref(&same), &more));
    // After a failed try or a limit the same reading says what they do not.
    assert!(is_news(&[same.clone(), failed(NOW)], &windows));
    assert!(is_news(&[same.clone(), limit(NOW, None, None)], &windows));
    // Only the end of the books counts.
    assert!(is_news(
        &[read(NOW - HOUR, more.clone()), same.clone()],
        &more
    ));
    assert!(!is_news(&[read(NOW - HOUR, more), same], &windows));
}

#[test]
fn windows_of_one_name_in_two_groups_each_have_their_own_rate() {
    // Grok's weekly credits and each product's share of them.
    let resets = NOW + 3 * 24 * HOUR;
    let week = |used: f64, group: Option<&str>| Window {
        group: group.map(Into::into),
        ..window("weekly", used, Some(resets))
    };
    let book = [
        read(
            NOW - 12 * HOUR - 60,
            vec![week(40.0, None), week(30.0, Some("Api"))],
        ),
        read(NOW - 60, vec![week(52.0, None), week(30.0, Some("Api"))]),
    ];
    let s = status(&book, NOW);
    let rates: Vec<_> = s
        .windows
        .iter()
        .map(|w| (w.group.clone(), w.rate))
        .collect();
    assert_eq!(rates, [(None, Some(1.0)), (Some("Api".into()), Some(0.0))]);
}
