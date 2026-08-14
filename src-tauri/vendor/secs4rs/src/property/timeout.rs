//! Timeout magnitude + unit, with float-seconds → milliseconds parity.
//!
//! Source: `Secs4Net.Local.Property.TimeoutAndUnit` /
//! `AbstractTimeoutProperty` / `Secs4Net.Jdk.TimeUnit`.

use std::time::Duration;

use super::cell::Property;

/// Time unit enum (names mirror Java `TimeUnit` for traceability).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeUnit {
    Nanoseconds,
    Microseconds,
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl TimeUnit {
    /// Convert `duration` in `self` to milliseconds (Java `TimeUnit.toMillis`).
    /// Saturates on overflow like the JDK/C# shim.
    pub fn to_millis(self, duration: i64) -> i64 {
        cvt(duration, SCALE_MS, scale_of(self))
    }

    /// Convert `duration` in `self` to a [`Duration`] for `std` waits.
    /// Negative or zero → zero-length duration (caller treats as no-wait).
    pub fn to_std_duration(self, duration: i64) -> Duration {
        if duration <= 0 {
            return Duration::ZERO;
        }
        let ms = self.to_millis(duration);
        if ms <= 0 {
            Duration::ZERO
        } else {
            Duration::from_millis(ms as u64)
        }
    }
}

const SCALE_NS: i64 = 1;
const SCALE_US: i64 = 1_000;
const SCALE_MS: i64 = 1_000_000;
const SCALE_S: i64 = 1_000_000_000;
const SCALE_MIN: i64 = SCALE_S * 60;
const SCALE_HR: i64 = SCALE_MIN * 60;
const SCALE_DAY: i64 = SCALE_HR * 24;

fn scale_of(u: TimeUnit) -> i64 {
    match u {
        TimeUnit::Nanoseconds => SCALE_NS,
        TimeUnit::Microseconds => SCALE_US,
        TimeUnit::Milliseconds => SCALE_MS,
        TimeUnit::Seconds => SCALE_S,
        TimeUnit::Minutes => SCALE_MIN,
        TimeUnit::Hours => SCALE_HR,
        TimeUnit::Days => SCALE_DAY,
    }
}

/// Java `TimeUnit.cvt` with long saturation.
fn cvt(d: i64, dst: i64, src: i64) -> i64 {
    if src == dst {
        return d;
    }
    if src < dst {
        return d / (dst / src);
    }
    let m = src / dst;
    let over = i64::MAX / m;
    if d > over {
        return i64::MAX;
    }
    if d < -over {
        return i64::MIN;
    }
    d * m
}

/// Pair of timeout magnitude + unit.
///
/// Source: `TimeoutAndUnit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeoutAndUnit {
    timeout: i64,
    unit: TimeUnit,
}

impl TimeoutAndUnit {
    /// Build from magnitude + unit. `timeout` must be `>= 0`.
    pub fn of(timeout: i64, unit: TimeUnit) -> Result<Self, TimeoutValueError> {
        if timeout < 0 {
            return Err(TimeoutValueError);
        }
        Ok(Self { timeout, unit })
    }

    /// Seconds (int) → SECONDS unit.
    pub fn of_seconds_i32(seconds: i32) -> Self {
        Self {
            timeout: i64::from(seconds),
            unit: TimeUnit::Seconds,
        }
    }

    /// Seconds (long) → SECONDS unit.
    pub fn of_seconds_i64(seconds: i64) -> Self {
        Self {
            timeout: seconds,
            unit: TimeUnit::Seconds,
        }
    }

    /// Float seconds → **milliseconds** magnitude.
    ///
    /// Parity: `(long)(seconds * 1000.0F)` + `TimeUnit.MILLISECONDS`
    /// (Secs4Net `TimeoutAndUnit.Of(float)`).
    pub fn of_seconds_f32(seconds: f32) -> Self {
        Self {
            timeout: (seconds * 1000.0_f32) as i64,
            unit: TimeUnit::Milliseconds,
        }
    }

    /// Double seconds → **microseconds** magnitude.
    ///
    /// Parity: `(long)(seconds * 1_000_000.0D)` + `TimeUnit.MICROSECONDS`.
    pub fn of_seconds_f64(seconds: f64) -> Self {
        Self {
            timeout: (seconds * 1_000_000.0_f64) as i64,
            unit: TimeUnit::Microseconds,
        }
    }

    pub fn timeout(self) -> i64 {
        self.timeout
    }

    pub fn unit(self) -> TimeUnit {
        self.unit
    }

    /// Converted milliseconds (`TimeoutAndUnit.MilliSeconds`).
    pub fn milli_seconds(self) -> i64 {
        self.unit.to_millis(self.timeout)
    }

    /// `std::time::Duration` for waits.
    pub fn as_duration(self) -> Duration {
        self.unit.to_std_duration(self.timeout)
    }
}

/// Negative timeout rejected (Java `IllegalArgumentException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutValueError;

impl std::fmt::Display for TimeoutValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "timeout must >=0L")
    }
}

impl std::error::Error for TimeoutValueError {}

/// Mutable timeout property (`TimeoutProperty`).
///
/// Stores [`TimeoutAndUnit`]; `set_seconds_f32` mirrors float-seconds config path
/// used by `SecsTimeout.T3(float)` etc.
#[derive(Clone)]
pub struct TimeoutProperty {
    inner: Property<TimeoutAndUnit>,
}

impl TimeoutProperty {
    pub fn new(initial: TimeoutAndUnit) -> Self {
        Self {
            inner: Property::new(initial),
        }
    }

    /// Default 0 ms (safe zero timeout).
    pub fn new_zero() -> Self {
        Self::new(TimeoutAndUnit {
            timeout: 0,
            unit: TimeUnit::Milliseconds,
        })
    }

    pub fn get(&self) -> TimeoutAndUnit {
        self.inner.get()
    }

    pub fn set(&self, value: TimeoutAndUnit) {
        self.inner.set(value);
    }

    pub fn set_seconds_i32(&self, seconds: i32) {
        self.inner.set(TimeoutAndUnit::of_seconds_i32(seconds));
    }

    pub fn set_seconds_i64(&self, seconds: i64) {
        self.inner.set(TimeoutAndUnit::of_seconds_i64(seconds));
    }

    /// Float seconds → internal ms (parity with `AbstractTimeoutProperty.Set(float)`).
    pub fn set_seconds_f32(&self, seconds: f32) {
        self.inner.set(TimeoutAndUnit::of_seconds_f32(seconds));
    }

    pub fn set_seconds_f64(&self, seconds: f64) {
        self.inner.set(TimeoutAndUnit::of_seconds_f64(seconds));
    }

    pub fn set_with_unit(&self, timeout: i64, unit: TimeUnit) -> Result<(), TimeoutValueError> {
        self.inner.set(TimeoutAndUnit::of(timeout, unit)?);
        Ok(())
    }

    pub fn as_property(&self) -> &Property<TimeoutAndUnit> {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_seconds_to_millis_parity() {
        // Secs4Net.Tests: T3(45F) → MilliSeconds = 45000
        let t = TimeoutAndUnit::of_seconds_f32(45.0);
        assert_eq!(t.unit(), TimeUnit::Milliseconds);
        assert_eq!(t.timeout(), 45_000);
        assert_eq!(t.milli_seconds(), 45_000);
    }

    #[test]
    fn float_fractional_truncates_toward_zero() {
        // (long)(0.0015F * 1000.0F) — float mul then cast
        let t = TimeoutAndUnit::of_seconds_f32(0.0015);
        assert_eq!(t.milli_seconds(), (0.0015_f32 * 1000.0_f32) as i64);
    }

    #[test]
    fn timeout_property_set_float() {
        let p = TimeoutProperty::new_zero();
        p.set_seconds_f32(45.0);
        assert_eq!(p.get().milli_seconds(), 45_000);
    }

    #[test]
    fn reject_negative() {
        assert!(TimeoutAndUnit::of(-1, TimeUnit::Seconds).is_err());
    }
}
