//! SECS timeouts T1–T8 (float seconds → milliseconds storage parity).
//!
//! Source: `SecsTimeout` / `SecsTimeoutImpl`.

use crate::property::{TimeoutAndUnit, TimeoutProperty};

/// T1–T8 timeout configuration (SEMI defaults in seconds).
#[derive(Clone)]
pub struct SecsTimeout {
    t1: TimeoutProperty,
    t2: TimeoutProperty,
    t3: TimeoutProperty,
    t4: TimeoutProperty,
    t5: TimeoutProperty,
    t6: TimeoutProperty,
    t7: TimeoutProperty,
    t8: TimeoutProperty,
}

impl Default for SecsTimeout {
    fn default() -> Self {
        Self::new()
    }
}

impl SecsTimeout {
    pub fn new() -> Self {
        Self {
            t1: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(1.0)),
            t2: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(15.0)),
            t3: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(45.0)),
            t4: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(45.0)),
            t5: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(10.0)),
            t6: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(5.0)),
            t7: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(10.0)),
            t8: TimeoutProperty::new(TimeoutAndUnit::of_seconds_f32(6.0)),
        }
    }

    pub fn t1(&self) -> &TimeoutProperty {
        &self.t1
    }
    pub fn set_t1(&self, seconds: f32) {
        self.t1.set_seconds_f32(seconds);
    }

    pub fn t2(&self) -> &TimeoutProperty {
        &self.t2
    }
    pub fn set_t2(&self, seconds: f32) {
        self.t2.set_seconds_f32(seconds);
    }

    pub fn t3(&self) -> &TimeoutProperty {
        &self.t3
    }
    pub fn set_t3(&self, seconds: f32) {
        self.t3.set_seconds_f32(seconds);
    }

    pub fn t4(&self) -> &TimeoutProperty {
        &self.t4
    }
    pub fn set_t4(&self, seconds: f32) {
        self.t4.set_seconds_f32(seconds);
    }

    pub fn t5(&self) -> &TimeoutProperty {
        &self.t5
    }
    pub fn set_t5(&self, seconds: f32) {
        self.t5.set_seconds_f32(seconds);
    }

    pub fn t6(&self) -> &TimeoutProperty {
        &self.t6
    }
    pub fn set_t6(&self, seconds: f32) {
        self.t6.set_seconds_f32(seconds);
    }

    pub fn t7(&self) -> &TimeoutProperty {
        &self.t7
    }
    pub fn set_t7(&self, seconds: f32) {
        self.t7.set_seconds_f32(seconds);
    }

    pub fn t8(&self) -> &TimeoutProperty {
        &self.t8
    }
    pub fn set_t8(&self, seconds: f32) {
        self.t8.set_seconds_f32(seconds);
    }
}
