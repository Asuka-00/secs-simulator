//! Boolean reactive property (protocol-path core).
//!
//! Source behavior: `BooleanProperty` / `AbstractBooleanProperty`.

use std::time::Duration;

use super::cell::{ListenerId, Property, WaitTimeout};
use super::timeout::{TimeUnit, TimeoutAndUnit};

/// Boolean property with get/set/wait (Secs4Net `BooleanProperty`).
#[derive(Clone)]
pub struct BooleanProperty {
    inner: Property<bool>,
}

impl BooleanProperty {
    /// `BooleanProperty.NewInstance(initial)`.
    pub fn new(initial: bool) -> Self {
        Self {
            inner: Property::new(initial),
        }
    }

    /// `BooleanValue()`.
    pub fn boolean_value(&self) -> bool {
        self.inner.get()
    }

    /// `Set(value)`.
    pub fn set(&self, value: bool) {
        self.inner.set(value);
    }

    /// `SetTrue()`.
    pub fn set_true(&self) {
        self.set(true);
    }

    /// `SetFalse()`.
    pub fn set_false(&self) {
        self.set(false);
    }

    /// `WaitUntil(condition)` — block until value equals condition.
    pub fn wait_until(&self, condition: bool) {
        self.inner.wait_until(|v| *v == condition);
    }

    /// `WaitUntilTrue()`.
    pub fn wait_until_true(&self) {
        self.wait_until(true);
    }

    /// `WaitUntilFalse()`.
    pub fn wait_until_false(&self) {
        self.wait_until(false);
    }

    /// `WaitUntil(condition, timeout, unit)`.
    pub fn wait_until_timeout(
        &self,
        condition: bool,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        let dur = unit.to_std_duration(timeout);
        self.inner
            .wait_until_timeout(|v| *v == condition, dur)
    }

    /// `WaitUntilTrue(timeout, unit)`.
    pub fn wait_until_true_timeout(
        &self,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        self.wait_until_timeout(true, timeout, unit)
    }

    /// `WaitUntilFalse(timeout, unit)`.
    pub fn wait_until_false_timeout(
        &self,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        self.wait_until_timeout(false, timeout, unit)
    }

    /// Wait using a [`TimeoutAndUnit`] (TimeoutGettable path simplified).
    pub fn wait_until_with_timeout_and_unit(
        &self,
        condition: bool,
        t: TimeoutAndUnit,
    ) -> Result<(), WaitTimeout> {
        self.wait_until_timeout(condition, t.timeout(), t.unit())
    }

    /// Convenience: wait true for a [`Duration`].
    pub fn wait_until_true_duration(&self, timeout: Duration) -> Result<(), WaitTimeout> {
        self.inner.wait_until_timeout(|v| *v, timeout)
    }

    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&bool) + Send + Sync + 'static,
    {
        self.inner.add_change_listener(listener)
    }

    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        self.inner.remove_change_listener(id)
    }

    pub fn as_property(&self) -> &Property<bool> {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn boolean_property_basic() {
        // Secs4Net.Tests: boolean-property-basic
        let p = BooleanProperty::new(false);
        assert!(!p.boolean_value());
        p.set_true();
        assert!(p.boolean_value());
        p.wait_until_true(); // already true → immediate
    }

    #[test]
    fn boolean_wait_until_true_threaded() {
        // Secs4Net.Tests: boolean-waitUntilTrue-threaded
        let p = BooleanProperty::new(false);
        let p2 = p.clone();
        let th = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            p2.set_true();
        });
        p.wait_until_true();
        assert!(p.boolean_value());
        th.join().unwrap();
    }

    #[test]
    fn boolean_wait_until_true_timeout() {
        // Secs4Net.Tests: boolean-waitUntilTrue-timeout
        let p = BooleanProperty::new(false);
        let r = p.wait_until_true_timeout(50, TimeUnit::Milliseconds);
        assert_eq!(r, Err(WaitTimeout));
    }
}
