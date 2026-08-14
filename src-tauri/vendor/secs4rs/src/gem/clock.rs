//! GEM Clock (SEMI-E30) A12 / A16 ASCII encoding.
//!
//! Source: `Secs4Net.Gem.Clock` / `AbstractClock` / `ClockType`.

use crate::secs2::{Error as Secs2Error, Result as Secs2Result, Secs2};

/// Clock wire format (`ClockType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClockType {
    /// A12 `yyMMddHHmmss`.
    A12,
    /// A16 `yyyyMMddHHmmss` + hundredths.
    #[default]
    A16,
}

/// Local civil time used by GEM clock (no timezone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalDateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Hundredths of a second (0..=99), A16 only.
    pub hundredths: u32,
}

impl LocalDateTime {
    pub fn new(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            hundredths: 0,
        }
    }
}

/// Immutable GEM clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clock {
    dt: LocalDateTime,
}

impl Clock {
    pub fn from_local(dt: LocalDateTime) -> Self {
        Self { dt }
    }

    pub fn to_local_date_time(&self) -> LocalDateTime {
        self.dt
    }

    /// A12: `yyMMddHHmmss` as SECS-II ASCII.
    pub fn to_ascii12(&self) -> Secs2Result<Secs2> {
        let yy = self.dt.year.rem_euclid(100);
        let s = format!(
            "{yy:02}{:02}{:02}{:02}{:02}{:02}",
            self.dt.month, self.dt.day, self.dt.hour, self.dt.minute, self.dt.second
        );
        Secs2::ascii(s)
    }

    /// A16: `yyyyMMddHHmmss` + hundredths (2 digits).
    pub fn to_ascii16(&self) -> Secs2Result<Secs2> {
        let s = format!(
            "{:04}{:02}{:02}{:02}{:02}{:02}{:02}",
            self.dt.year,
            self.dt.month,
            self.dt.day,
            self.dt.hour,
            self.dt.minute,
            self.dt.second,
            self.dt.hundredths.min(99)
        );
        Secs2::ascii(s)
    }

    /// Encode per [`ClockType`] (`GetClockSecs2`).
    pub fn to_secs2(&self, clock_type: ClockType) -> Secs2Result<Secs2> {
        match clock_type {
            ClockType::A12 => self.to_ascii12(),
            ClockType::A16 => self.to_ascii16(),
        }
    }

    /// Parse from SECS-II ASCII body (12 or 16 chars).
    pub fn from_secs2(secs2: &Secs2) -> Secs2Result<Self> {
        let a = secs2.get_ascii()?;
        let len = a.len();
        if len == 12 {
            let yyyy = get_year(&a[0..2])?;
            let mm = parse_u32(&a[2..4])?;
            let dd = parse_u32(&a[4..6])?;
            let hh = parse_u32(&a[6..8])?;
            let ii = parse_u32(&a[8..10])?;
            let ss = parse_u32(&a[10..12])?;
            Ok(Self::from_local(LocalDateTime::new(
                yyyy, mm, dd, hh, ii, ss,
            )))
        } else if len == 16 {
            let yyyy = parse_i32(&a[0..4])?;
            let mm = parse_u32(&a[4..6])?;
            let dd = parse_u32(&a[6..8])?;
            let hh = parse_u32(&a[8..10])?;
            let ii = parse_u32(&a[10..12])?;
            let ss = parse_u32(&a[12..14])?;
            let hundredths = parse_u32(&a[14..16])?;
            let mut dt = LocalDateTime::new(yyyy, mm, dd, hh, ii, ss);
            dt.hundredths = hundredths;
            Ok(Self::from_local(dt))
        } else {
            Err(Secs2Error::IllegalDataFormat("Clock parse length"))
        }
    }
}

fn parse_u32(s: &str) -> Secs2Result<u32> {
    s.parse()
        .map_err(|_| Secs2Error::IllegalDataFormat("Clock parse int"))
}

fn parse_i32(s: &str) -> Secs2Result<i32> {
    s.parse()
        .map_err(|_| Secs2Error::IllegalDataFormat("Clock parse int"))
}

/// 2-digit year → full year with century boundary handling (AbstractClock.GetYear).
fn get_year(a2: &str) -> Secs2Result<i32> {
    let yy = parse_i32(a2)?;
    let now_year = current_year();
    let century = (now_year / 100) * 100;
    let flac = now_year % 100;
    if flac < 25 {
        if yy >= 75 {
            return Ok(century - 100 + yy);
        }
    } else if flac >= 75 {
        if yy < 25 {
            return Ok(century + 100 + yy);
        }
    }
    Ok(century + yy)
}

fn current_year() -> i32 {
    // Use OS local time year without extra crates.
    // Parity: century/flac from "now" at parse time (same as C# static init on first use).
    use std::time::{SystemTime, UNIX_EPOCH};
    // Approximate: not needed for A16 roundtrip tests; for A12 year expand only.
    // Fall back: 2026 if conversion fails (matches workspace date context).
    let Ok(dur) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return 2026;
    };
    // Civil year approx via days (good enough for century boundary tests; A16 doesn't use this).
    // Use a simple algorithm: days since 1970-01-01 → year.
    let days = (dur.as_secs() / 86400) as i64;
    days_to_year(days)
}

fn days_to_year(mut days: i64) -> i32 {
    // days since 1970-01-01
    let mut year = 1970_i32;
    loop {
        let diy = if is_leap(year) { 366 } else { 365 };
        if days >= diy {
            days -= diy;
            year += 1;
        } else {
            break;
        }
    }
    year
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_ascii12() {
        let c = Clock::from_local(LocalDateTime::new(2026, 6, 16, 13, 45, 30));
        assert_eq!(c.to_ascii12().unwrap().get_ascii().unwrap(), "260616134530");
    }

    #[test]
    fn clock_ascii16() {
        let c = Clock::from_local(LocalDateTime::new(2026, 6, 16, 13, 45, 30));
        assert_eq!(
            c.to_ascii16().unwrap().get_ascii().unwrap(),
            "2026061613453000"
        );
    }

    #[test]
    fn clock_roundtrip_from_secs2() {
        let dt = LocalDateTime::new(2025, 12, 31, 23, 59, 58);
        let c = Clock::from_local(dt);
        let back = Clock::from_secs2(&c.to_ascii16().unwrap()).unwrap();
        assert_eq!(dt, back.to_local_date_time());
    }
}
