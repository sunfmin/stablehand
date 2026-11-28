//! Time as stablehand says and reads it: seconds since the Unix epoch, and
//! UTC RFC 3339 (`2026-11-24T08:30:00Z`). No clock here: the caller hands
//! in `now`.

/// Days from 1970-01-01 to the civil date, proleptic Gregorian (Howard
/// Hinnant's days_from_civil).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `secs` after the Unix epoch as UTC RFC 3339 (`2026-11-24T08:30:00Z`).
pub fn utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil_from_days, for days since 1970-01-01.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// Seconds from `now` until `at`: none where `at` is not said, or past.
pub fn until(at: Option<u64>, now: u64) -> Option<u64> {
    at.filter(|at| *at > now).map(|at| at - now)
}

/// `secs` as a person reads a span, in its two largest units, a zero
/// second one left out, and seconds only under a minute: `6d 6h`,
/// `4h 12m`, `12m`, `45s`.
pub fn span(secs: u64) -> String {
    let units = [(86_400, "d"), (3_600, "h"), (60, "m"), (1, "s")];
    let i = units.iter().position(|(u, _)| secs >= *u).unwrap_or(3);
    let (big, b) = units[i];
    let mut said = format!("{}{b}", secs / big);
    if let Some(&(small, s)) = units.get(i + 1).filter(|(u, _)| *u >= 60)
        && secs % big / small > 0
    {
        said.push_str(&format!(" {}{s}", secs % big / small));
    }
    said
}

/// An RFC 3339 time (`2026-11-24T10:30:00Z`, `2026-11-24T10:30:00.029137+00:00`)
/// as seconds since the Unix epoch, a fraction dropped; none where it is no
/// such time or is before 1970.
pub fn parse(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let p = s.get(r)?;
        p.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| p.parse().ok())?
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &frac[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let o = rest.as_bytes();
            if o.len() != 6 || o[3] != b':' {
                return None;
            }
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6)?.parse().ok()?;
            sign * (oh * 3600 + om * 60)
        }
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=days).contains(&d) || h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset;
    u64::try_from(secs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_says_the_calendar_time() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_825_599), "2000-02-29T11:59:59Z");
        assert_eq!(utc(1_795_516_200), "2026-11-24T10:30:00Z");
        assert_eq!(utc(4_107_542_399), "2100-02-28T23:59:59Z");
    }

    #[test]
    fn span_says_its_two_largest_units() {
        assert_eq!(span(6 * 86_400 + 6 * 3_600 + 59), "6d 6h");
        assert_eq!(span(4 * 3_600 + 12 * 60 + 5), "4h 12m");
        assert_eq!(span(12 * 60 + 30), "12m");
        assert_eq!(span(2 * 86_400), "2d");
        assert_eq!(span(3_600 + 30), "1h");
        assert_eq!(span(45), "45s");
        assert_eq!(span(0), "0s");
        assert_eq!(until(Some(100), 40), Some(60));
        assert_eq!(until(Some(40), 40), None);
        assert_eq!(until(None, 40), None);
    }

    #[test]
    fn parse_reads_what_utc_writes_and_what_engines_answer() {
        for secs in [0, 951_825_599, 1_795_516_200, 4_107_542_399] {
            assert_eq!(parse(&utc(secs)), Some(secs));
        }
        // claude-code's own form: micros and an offset.
        assert_eq!(
            parse("2026-11-24T10:30:00.029137+00:00"),
            Some(1_795_516_200)
        );
        assert_eq!(parse("2026-11-24T19:30:00+09:00"), Some(1_795_516_200));
        assert_eq!(parse("2026-11-24T05:00:00-05:30"), Some(1_795_516_200));
        for bad in [
            "",
            "2026-11-24",
            "2026-11-24T10:30:00",
            "2026-11-24T10:30:00.Z",
            "2026-11-24T24:00:00Z",
            "2026-02-29T10:30:00Z",
            "2026-11-24T10:30:00+0900",
            "2026-11-24T10:30:00X",
            "2026-11-24T10-30:00Z",
            "2026-11-24T19:30:00+09X00",
            "2100-02-29T00:00:00Z",
            "2026-11x24T10:30:00Z",
            "2026x11-24T10:30:00Z",
            "2026-11-24T10:30-00Z",
            "1969-12-31T23:59:59Z",
            "2026-1a-24T10:30:00Z",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
