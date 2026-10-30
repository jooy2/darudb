//! Dates as the query language writes them, `DATE "2026-10-30T12:00:00Z"`:
//! RFC 3339 text and the microseconds since the Unix epoch a date holds
//! (`design/objects.md`, "The query language").
//!
//! The text is a date and a time with seconds, up to six digits of a second,
//! and `Z` or an offset from UTC, or a date alone, which is its midnight in
//! UTC. The calendar is the proleptic Gregorian one, years 0000 to 9999 as
//! RFC 3339 has them. A leap second is refused: a date counts every day as
//! 86,400 seconds, as Unix time does.

/// Microseconds in a day.
const DAY: i64 = 86_400_000_000;

/// The microseconds since the Unix epoch that `text` names, or why it names
/// none.
pub(crate) fn parse(text: &str) -> Result<i64, &'static str> {
    let bytes = text.as_bytes();
    let year = digits(bytes, 0, 4)?;

    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return Err("a date is written YYYY-MM-DD");
    }

    let month = digits(bytes, 5, 2)?;
    let day = digits(bytes, 8, 2)?;

    if !(1..=12).contains(&month) || day == 0 || day > days_in(year, month) {
        return Err("a date names a day that is not in the calendar");
    }

    let days = days_from_civil(year, month, day);

    if bytes.len() == 10 {
        return Ok(days * DAY);
    }

    if !matches!(bytes.get(10), Some(b'T' | b't')) {
        return Err("a date's time follows a `T`");
    }

    let hour = digits(bytes, 11, 2)?;
    let minute = digits(bytes, 14, 2)?;
    let second = digits(bytes, 17, 2)?;

    if bytes.get(13) != Some(&b':') || bytes.get(16) != Some(&b':') {
        return Err("a date's time is written HH:MM:SS");
    }

    if hour > 23 || minute > 59 || second > 59 {
        return Err("a date names a time that is not on the clock, or a leap second");
    }

    let mut at = 19;
    let mut micros = 0;

    if bytes.get(at) == Some(&b'.') {
        let start = at + 1;

        at = start;

        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }

        let places = at - start;

        if places == 0 || places > 6 {
            return Err("a date's fraction of a second has one to six digits");
        }

        micros = digits(bytes, start, places)? * 10_i64.pow(u32::try_from(6 - places).unwrap_or(0));
    }

    let offset = match bytes.get(at..) {
        Some([b'Z' | b'z']) => 0,
        Some([sign @ (b'+' | b'-'), _, _, b':', _, _]) => {
            let hours = digits(bytes, at + 1, 2)?;
            let minutes = digits(bytes, at + 4, 2)?;

            if hours > 23 || minutes > 59 {
                return Err("a date's offset from UTC is not one");
            }

            let offset = (hours * 60 + minutes) * 60_000_000;

            if *sign == b'-' { -offset } else { offset }
        }
        _ => return Err("a date's time ends in `Z` or an offset from UTC such as `+09:00`"),
    };
    let time = ((hour * 60 + minute) * 60 + second) * 1_000_000 + micros;

    // Years 0000 to 9999 lie well inside an `i64` of microseconds.
    Ok(days * DAY + time - offset)
}

/// `micros`, a date, as the query language writes it: in UTC, with as many
/// digits of a second as it has, and none for a whole second. A date outside
/// the years 0000 to 9999 has no such text.
#[cfg(test)]
pub(crate) fn text(micros: i64) -> Option<String> {
    let days = micros.div_euclid(DAY);
    let time = micros.rem_euclid(DAY);
    let (year, month, day) = civil_from_days(days);

    if !(0..=9999).contains(&year) {
        return None;
    }

    let seconds = time / 1_000_000;
    let fraction = time % 1_000_000;
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );

    if fraction != 0 {
        let digits = format!("{fraction:06}");

        text.push('.');
        text.push_str(digits.trim_end_matches('0'));
    }

    text.push('Z');

    Some(text)
}

/// The number written in the `len` ASCII digits at `at` of `bytes`.
fn digits(bytes: &[u8], at: usize, len: usize) -> Result<i64, &'static str> {
    let digits = bytes
        .get(at..at + len)
        .filter(|digits| digits.iter().all(u8::is_ascii_digit))
        .ok_or("a date is written as RFC 3339 has it, such as 2026-10-30T12:00:00Z")?;

    Ok(digits
        .iter()
        .fold(0, |number, digit| number * 10 + i64::from(digit - b'0')))
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The days from 1970-01-01 to the day `year`-`month`-`day`, by counting
/// in eras of 400 years from a year that starts in March, so that the leap
/// day comes last.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
}

/// The year, month and day that are `days` from 1970-01-01, as
/// [`days_from_civil`] counts them.
#[cfg(test)]
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);

    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_their_microseconds_since_the_epoch() {
        for (text, micros) in [
            ("1970-01-01", 0),
            ("1970-01-01T00:00:00Z", 0),
            ("1970-01-01T00:00:00.000001Z", 1),
            ("1969-12-31T23:59:59.999999Z", -1),
            ("1970-01-02", DAY),
            ("2000-02-29T12:00:00Z", 951_825_600_000_000),
            ("2026-10-30T21:00:00+09:00", 1_793_361_600_000_000),
            ("2026-10-30T12:00:00Z", 1_793_361_600_000_000),
            ("2026-10-30t12:00:00z", 1_793_361_600_000_000),
            ("2026-10-30T07:30:00-04:30", 1_793_361_600_000_000),
            ("2026-10-30T12:00:00.5Z", 1_793_361_600_500_000),
            ("0000-01-01", -62_167_219_200_000_000),
            ("9999-12-31T23:59:59.999999Z", 253_402_300_799_999_999),
        ] {
            assert_eq!(parse(text), Ok(micros), "{text}");
        }
    }

    #[test]
    fn text_that_is_not_a_date_is_refused() {
        for text in [
            "",
            "2026",
            "2026-1-30",
            "2026-13-01",
            "2026-02-29",
            "1900-02-29",
            "2026-10-00",
            "2026-10-30T",
            "2026-10-30T12:00Z",
            "2026-10-30T24:00:00Z",
            "2026-10-30T23:59:60Z",
            "2026-10-30T12:00:00",
            "2026-10-30T12:00:00.Z",
            "2026-10-30T12:00:00.1234567Z",
            "2026-10-30T12:00:00+0900",
            "2026-10-30T12:00:00+24:00",
            "2026-10-30 12:00:00Z",
            "2026-10-30T12:00:00Zz",
            "+2026-10-30",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn every_day_reads_back_as_it_is_written() {
        // A day in each 97 for the whole range, and the days around the
        // epoch and the leap days of 2000 and 1900.
        let first = days_from_civil(0, 1, 1);
        let last = days_from_civil(9999, 12, 31);
        let days = (first..=last)
            .step_by(97)
            .chain(-3..3)
            .chain(days_from_civil(2000, 2, 28)..days_from_civil(2000, 3, 2))
            .chain(days_from_civil(1900, 2, 28)..days_from_civil(1900, 3, 2));

        for days in days {
            let (year, month, day) = civil_from_days(days);

            assert_eq!(days_from_civil(year, month, day), days);

            for micros in [
                days * DAY,
                days * DAY + 1,
                days * DAY + DAY - 1,
                days * DAY + 1_500,
            ] {
                let text = text(micros).unwrap();

                assert_eq!(parse(&text), Ok(micros), "{text}");
            }
        }

        assert_eq!(text(days_from_civil(10_000, 1, 1) * DAY), None);
        assert_eq!(text(-1).as_deref(), Some("1969-12-31T23:59:59.999999Z"));
        assert_eq!(text(1_500).as_deref(), Some("1970-01-01T00:00:00.0015Z"));
    }
}
