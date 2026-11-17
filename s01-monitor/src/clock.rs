//! The battery clock: this device's local date and time, the frame that sets
//! the battery's clock, and what the battery's own clock frame (`0x200`) says.
//!
//! Setting the clock is the one thing the app writes to the scooter. It is
//! never done by itself: a person presses a button and confirms. The frame is
//! the "Set current time" frame (`0x201`,
//! seven bytes: `04`, then hour, minute, second, day, month and two-digit year
//! as BCD) and it was tried on the scooter on 9 Nov 2026 (CAN_NOTES.md).
//!
//! The local time comes from the device with no extra crate: `localtime_r` on a
//! Mac or Linux computer, `GetLocalTime` on Windows, and the page's own date in
//! a browser. The battery has no time zone, so it is set to the local time of
//! the moment, and it stays an hour out after a clock change until it is set
//! again.

/// A date and time with no time zone: what the clocks show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Moment {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

fn leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days from 1970-01-01 to this day (the usual civil-calendar count).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

impl Moment {
    /// Whether this is a real date and time between 2000 and 2099, the years a
    /// two-digit year can mean here.
    pub fn is_valid(&self) -> bool {
        (2000..=2099).contains(&self.year)
            && (1..=12).contains(&self.month)
            && (1..=days_in_month(self.year, self.month)).contains(&self.day)
            && self.hour <= 23
            && self.minute <= 59
            && self.second <= 59
    }

    /// `2026-11-09 22:28:04`, which no language or country reads two ways.
    pub fn text(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// Seconds since 2000-01-01 00:00:00. There is no time zone in it, so it is
    /// only good for the difference between two moments.
    pub fn seconds(&self) -> i64 {
        let days = days_from_civil(i64::from(self.year), i64::from(self.month), i64::from(self.day))
            - days_from_civil(2000, 1, 1);
        days * 86_400 + i64::from(self.hour) * 3600 + i64::from(self.minute) * 60 + i64::from(self.second)
    }

    /// The line that sets the battery clock to this moment, or `None` if it is
    /// not a date the frame can hold. The fields are written as their decimal
    /// text read as hex, which gives the BCD.
    pub fn set_line(&self) -> Option<String> {
        if !self.is_valid() {
            return None;
        }
        Some(format!(
            "t201704{:02}{:02}{:02}{:02}{:02}{:02}\r",
            self.hour,
            self.minute,
            self.second,
            self.day,
            self.month,
            self.year % 100
        ))
    }
}

fn bcd(byte: u8) -> Option<u8> {
    let (tens, ones) = (byte >> 4, byte & 0x0F);
    (tens <= 9 && ones <= 9).then_some(tens * 10 + ones)
}

/// The battery's own clock frame (`0x200`): byte 0 bit 1 says the clock is
/// valid, then BCD hour, minute, second, day, month and two-digit year. `None`
/// when the clock is not marked valid or any byte is not a date.
pub fn from_frame(data: &[u8]) -> Option<Moment> {
    if data.len() < 7 || data[0] & 0x02 == 0 {
        return None;
    }
    let moment = Moment {
        hour: bcd(data[1])?,
        minute: bcd(data[2])?,
        second: bcd(data[3])?,
        day: bcd(data[4])?,
        month: bcd(data[5])?,
        year: 2000 + u16::from(bcd(data[6])?),
    };
    moment.is_valid().then_some(moment)
}

/// What the app says about the last request to set the battery clock. The first
/// two are Wording lines in CAN_NOTES.md (`0x201`).
pub const NOTE_MATCHES: &str = "The battery clock now matches this device's time.";
pub const NOTE_DIFFERS: &str = "The battery clock still does not match this device's time.";
pub const NOTE_SETTING: &str = "Setting the battery clock.";
pub const NOTE_NO_TIME: &str = "This device's clock could not be read, so nothing was sent.";

/// How long after the request the battery's clock must show the new time, and how
/// many seconds out it may be (its frame comes about once a second).
pub const CHECK_WAIT_S: u64 = 3;
pub const CHECK_TOLERANCE_S: i64 = 3;

/// How far a clock is from another, in words: "4 min 12 s", "3 h", "9,121
/// days". Positive or negative, as `seconds`.
pub fn span_text(seconds: i64) -> String {
    let total = seconds.unsigned_abs();
    if total >= 2 * 86_400 {
        let days = total / 86_400;
        let digits = days.to_string();
        let mut grouped = String::new();
        for (place, digit) in digits.chars().enumerate() {
            if place > 0 && (digits.len() - place).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(digit);
        }
        return format!("{grouped} days");
    }
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{hours} h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes} min"));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{secs} s"));
    }
    parts.join(" ")
}

/// This device's local date and time, or `None` if it cannot be read.
#[cfg(target_arch = "wasm32")]
pub fn local_now() -> Option<Moment> {
    let now = js_sys::Date::new_0();
    let moment = Moment {
        year: now.get_full_year() as u16,
        month: now.get_month() as u8 + 1,
        day: now.get_date() as u8,
        hour: now.get_hours() as u8,
        minute: now.get_minutes() as u8,
        second: now.get_seconds() as u8,
    };
    moment.is_valid().then_some(moment)
}

#[cfg(unix)]
pub fn local_now() -> Option<Moment> {
    // SAFETY: `time` and `localtime_r` are the C library's own, and `tm` is
    // zeroed memory that `localtime_r` fills in. A null answer is checked.
    let (seconds, tm) = unsafe {
        let seconds = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&seconds, &mut tm).is_null() {
            return None;
        }
        (seconds, tm)
    };
    let _ = seconds;
    let moment = Moment {
        year: u16::try_from(tm.tm_year + 1900).ok()?,
        month: u8::try_from(tm.tm_mon + 1).ok()?,
        day: u8::try_from(tm.tm_mday).ok()?,
        hour: u8::try_from(tm.tm_hour).ok()?,
        minute: u8::try_from(tm.tm_min).ok()?,
        // 60 is a leap second; the battery has none.
        second: u8::try_from(tm.tm_sec.min(59)).ok()?,
    };
    moment.is_valid().then_some(moment)
}

#[cfg(windows)]
pub fn local_now() -> Option<Moment> {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut time = SystemTime::default();
    // SAFETY: `GetLocalTime` fills in the eight 16-bit fields it is given.
    unsafe { GetLocalTime(&mut time) };
    let moment = Moment {
        year: time.year,
        month: u8::try_from(time.month).ok()?,
        day: u8::try_from(time.day).ok()?,
        hour: u8::try_from(time.hour).ok()?,
        minute: u8::try_from(time.minute).ok()?,
        second: u8::try_from(time.second.min(59)).ok()?,
    };
    moment.is_valid().then_some(moment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moment(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Moment {
        Moment { year, month, day, hour, minute, second }
    }

    #[test]
    fn the_set_line_is_the_frame_that_was_sent_on_9_nov_2026() {
        // It was sent as this, the battery accepted it, and its clock changed.
        assert_eq!(moment(2026, 11, 9, 22, 28, 3).set_line().as_deref(), Some("t201704222803091126\r"));
        assert_eq!(moment(2026, 1, 2, 3, 4, 5).set_line().as_deref(), Some("t201704030405020126\r"));
        assert_eq!(moment(2099, 12, 31, 23, 59, 59).set_line().as_deref(), Some("t201704235959311299\r"));
    }

    #[test]
    fn a_date_the_frame_cannot_hold_gives_no_line() {
        for bad in [
            moment(1999, 12, 31, 0, 0, 0),
            moment(2100, 1, 1, 0, 0, 0),
            moment(2026, 0, 1, 0, 0, 0),
            moment(2026, 13, 1, 0, 0, 0),
            moment(2026, 11, 31, 0, 0, 0),
            moment(2026, 2, 29, 0, 0, 0),
            moment(2026, 11, 0, 0, 0, 0),
            moment(2026, 11, 9, 24, 0, 0),
            moment(2026, 11, 9, 0, 60, 0),
            moment(2026, 11, 9, 0, 0, 60),
        ] {
            assert_eq!(bad.set_line(), None, "{bad:?}");
        }
        assert!(moment(2028, 2, 29, 12, 0, 0).is_valid(), "2028 is a leap year");
        assert!(!moment(2100, 2, 29, 12, 0, 0).is_valid());
    }

    #[test]
    fn every_line_the_app_can_make_is_a_well_formed_clock_frame() {
        // The same shape the firmware's filter accepts: id 201, seven bytes, 04 first.
        for line in [moment(2026, 11, 9, 22, 28, 3), moment(2000, 1, 1, 0, 0, 0), moment(2099, 12, 31, 23, 59, 59)]
            .iter()
            .filter_map(Moment::set_line)
        {
            assert!(line.starts_with("t201704") && line.ends_with('\r') && line.len() == 7 + 12 + 1, "{line:?}");
            assert!(line[7..line.len() - 1].chars().all(|digit| digit.is_ascii_digit()), "{line:?}");
        }
    }

    #[test]
    fn the_battery_clock_frame_is_read_as_bcd_when_it_is_marked_valid() {
        // As on the scooter on 9 Nov 2026: 22:28:04 on 09/11/26, then 11:30:30 on 18/01/01.
        assert_eq!(from_frame(&[0x03, 0x22, 0x28, 0x04, 0x09, 0x11, 0x26]), Some(moment(2026, 11, 9, 22, 28, 4)));
        assert_eq!(from_frame(&[0x03, 0x11, 0x30, 0x30, 0x18, 0x01, 0x01]), Some(moment(2001, 1, 18, 11, 30, 30)));
        // An eighth byte is allowed.
        assert!(from_frame(&[0x03, 0x22, 0x28, 0x04, 0x09, 0x11, 0x26, 0x00]).is_some());
        // Not marked valid, too short, not BCD, not a date.
        assert_eq!(from_frame(&[0x01, 0x22, 0x28, 0x04, 0x09, 0x11, 0x26]), None);
        assert_eq!(from_frame(&[0x03, 0x22, 0x28, 0x04, 0x09, 0x11]), None);
        assert_eq!(from_frame(&[0x03, 0x2A, 0x28, 0x04, 0x09, 0x11, 0x26]), None);
        assert_eq!(from_frame(&[0x03, 0x22, 0x28, 0x04, 0x09, 0x13, 0x26]), None);
        assert_eq!(from_frame(&[0x03, 0x22, 0x28, 0x04, 0x31, 0x11, 0x26]), None, "31 Nov");
    }

    #[test]
    fn the_difference_between_two_clocks_is_in_seconds() {
        let a = moment(2026, 11, 9, 22, 28, 4);
        assert_eq!(moment(2026, 11, 9, 22, 28, 9).seconds() - a.seconds(), 5);
        assert_eq!(moment(2026, 11, 10, 0, 0, 0).seconds() - a.seconds(), 5516);
        assert_eq!(moment(2026, 12, 1, 0, 0, 0).seconds() - moment(2026, 11, 1, 0, 0, 0).seconds(), 30 * 86_400);
        assert_eq!(moment(2028, 3, 1, 0, 0, 0).seconds() - moment(2028, 2, 28, 0, 0, 0).seconds(), 2 * 86_400, "a leap day");
        assert_eq!(moment(2000, 1, 1, 0, 0, 0).seconds(), 0);
        // The battery in January 2001 against 9 Nov 2026.
        let behind = moment(2001, 1, 18, 11, 30, 30).seconds() - a.seconds();
        assert_eq!(span_text(behind), "9,426 days");
    }

    #[test]
    fn spans_are_said_in_plain_units() {
        assert_eq!(span_text(0), "0 s");
        assert_eq!(span_text(-1), "1 s");
        assert_eq!(span_text(59), "59 s");
        assert_eq!(span_text(60), "1 min");
        assert_eq!(span_text(-(5 * 60 + 12)), "5 min 12 s");
        assert_eq!(span_text(3 * 3600), "3 h");
        assert_eq!(span_text(10 * 3600 + 29 * 60 + 51), "10 h 29 min 51 s");
        assert_eq!(span_text(2 * 86_400), "2 days");
        assert_eq!(span_text(1_000 * 86_400), "1,000 days");
        assert_eq!(span_text(1_000_000 * 86_400), "1,000,000 days");
    }

    #[test]
    fn this_devices_local_time_is_a_real_date_and_matches_the_system_clock() {
        let now = local_now().expect("the local time");
        assert!(now.is_valid() && now.year >= 2026, "{now:?}");
        // Ask the system's own `date` and compare, to within a few seconds.
        #[cfg(unix)]
        {
            let output = std::process::Command::new("date").arg("+%Y-%m-%d %H:%M:%S").output().expect("date");
            let text = String::from_utf8(output.stdout).unwrap();
            let text = text.trim();
            let number = |range: std::ops::Range<usize>| text[range].parse::<u8>().unwrap();
            let from_date = moment(
                text[0..4].parse().unwrap(),
                number(5..7),
                number(8..10),
                number(11..13),
                number(14..16),
                number(17..19),
            );
            let gap = (local_now().unwrap().seconds() - from_date.seconds()).abs();
            assert!(gap <= 5, "local_now is {gap} s from date: {now:?} against {text}");
        }
    }
}
