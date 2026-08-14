//! Integer reactive property (protocol-path + oracle compute/arithmetic).
//!
//! Source: `IntegerProperty` / `AbstractIntegerProperty` / `NumberObservable`.
//! Comparative / arithmetic results are live [`BooleanProperty`] / [`IntegerProperty`]
//! (idiomatic stand-in for `ComparativeCompution` / `NumberCompution`).

use std::time::Duration;

use super::boolean::BooleanProperty;
use super::cell::{ListenerId, Property, WaitTimeout};
use super::timeout::TimeUnit;

/// Integer property (`IntegerProperty`).
#[derive(Clone)]
pub struct IntegerProperty {
    inner: Property<i32>,
}

impl IntegerProperty {
    /// `IntegerProperty.NewInstance(initial)`.
    pub fn new(initial: i32) -> Self {
        Self {
            inner: Property::new(initial),
        }
    }

    /// `IntValue()`.
    pub fn int_value(&self) -> i32 {
        self.inner.get()
    }

    /// `Set(value)`.
    pub fn set(&self, value: i32) {
        self.inner.set(value);
    }

    /// Block until value equals `expected` (Number wait path used by tests).
    pub fn wait_until_equal_to(&self, expected: i32) {
        self.inner.wait_until_equal_to(&expected);
    }

    pub fn wait_until_equal_to_timeout(
        &self,
        expected: i32,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_equal_to_timeout(&expected, unit.to_std_duration(timeout))
    }

    pub fn wait_until_greater_than(&self, threshold: i32) {
        self.inner.wait_until(|v| *v > threshold);
    }

    pub fn wait_until_greater_than_timeout(
        &self,
        threshold: i32,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_timeout(|v| *v > threshold, unit.to_std_duration(timeout))
    }

    pub fn wait_until_equal_to_duration(
        &self,
        expected: i32,
        timeout: Duration,
    ) -> Result<(), WaitTimeout> {
        self.inner.wait_until_equal_to_timeout(&expected, timeout)
    }

    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&i32) + Send + Sync + 'static,
    {
        self.inner.add_change_listener(listener)
    }

    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        self.inner.remove_change_listener(id)
    }

    pub fn as_property(&self) -> &Property<i32> {
        &self.inner
    }

    /// `ComputeIsEqualTo(n)` — live boolean bound to `value == n`.
    pub fn compute_is_equal_to(&self, n: i32) -> BooleanProperty {
        let bp = BooleanProperty::new(self.int_value() == n);
        let bp2 = bp.clone();
        self.add_change_listener(move |v| bp2.set(*v == n));
        bp
    }

    /// `ComputeIsGreaterThan(n)`.
    pub fn compute_is_greater_than(&self, n: i32) -> BooleanProperty {
        let bp = BooleanProperty::new(self.int_value() > n);
        let bp2 = bp.clone();
        self.add_change_listener(move |v| bp2.set(*v > n));
        bp
    }

    /// `ComputeIsLessThan(n)`.
    pub fn compute_is_less_than(&self, n: i32) -> BooleanProperty {
        let bp = BooleanProperty::new(self.int_value() < n);
        let bp2 = bp.clone();
        self.add_change_listener(move |v| bp2.set(*v < n));
        bp
    }

    /// `Add(other)` — live sum of two integer properties.
    pub fn add(&self, other: &IntegerProperty) -> IntegerProperty {
        let sum = IntegerProperty::new(self.int_value().wrapping_add(other.int_value()));
        let sum_a = sum.clone();
        let other_a = other.clone();
        self.add_change_listener(move |a| {
            sum_a.set(a.wrapping_add(other_a.int_value()));
        });
        let sum_b = sum.clone();
        let self_b = self.clone();
        other.add_change_listener(move |b| {
            sum_b.set(self_b.int_value().wrapping_add(*b));
        });
        sum
    }

    /// `Subtract(other)` — live difference `self - other`.
    pub fn subtract(&self, other: &IntegerProperty) -> IntegerProperty {
        let diff = IntegerProperty::new(self.int_value().wrapping_sub(other.int_value()));
        let diff_a = diff.clone();
        let other_a = other.clone();
        self.add_change_listener(move |a| {
            diff_a.set(a.wrapping_sub(other_a.int_value()));
        });
        let diff_b = diff.clone();
        let self_b = self.clone();
        other.add_change_listener(move |b| {
            diff_b.set(self_b.int_value().wrapping_sub(*b));
        });
        diff
    }

    /// `Negate()` — live `-value`.
    pub fn negate(&self) -> IntegerProperty {
        let n = IntegerProperty::new(self.int_value().wrapping_neg());
        let n2 = n.clone();
        self.add_change_listener(move |v| n2.set(v.wrapping_neg()));
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn integer_property_basic() {
        // Secs4Net.Tests: integer-property-basic
        let p = IntegerProperty::new(7);
        assert_eq!(p.int_value(), 7);
        p.set(42);
        assert_eq!(p.int_value(), 42);
    }

    #[test]
    fn integer_wait_until_equal_threaded() {
        // Secs4Net.Tests: integer-waitUntilEqualTo-threaded (Batch4)
        let p = IntegerProperty::new(0);
        let p2 = p.clone();
        let th = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            p2.set(99);
        });
        p.wait_until_equal_to(99);
        assert_eq!(p.int_value(), 99);
        th.join().unwrap();
    }

    #[test]
    fn integer_wait_until_greater_than_threaded() {
        let p = IntegerProperty::new(0);
        let p2 = p.clone();
        let th = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            p2.set(6);
        });
        p.wait_until_greater_than(5);
        assert!(p.int_value() > 5);
        th.join().unwrap();
    }

    #[test]
    fn number_compute_is_equal_to_reactive() {
        // Secs4Net.Tests: number-compute-isEqualTo-reactive
        let p = IntegerProperty::new(5);
        let eq = p.compute_is_equal_to(5);
        assert!(eq.boolean_value(), "5==5");
        p.set(6);
        assert!(!eq.boolean_value(), "6==5");
        p.set(5);
        assert!(eq.boolean_value(), "back to 5==5");
    }

    #[test]
    fn number_compute_is_greater_than_reactive() {
        // Secs4Net.Tests: number-compute-isGreaterThan-reactive
        let p = IntegerProperty::new(3);
        let gt = p.compute_is_greater_than(5);
        assert!(!gt.boolean_value());
        p.set(10);
        assert!(gt.boolean_value());
    }

    #[test]
    fn number_arithmetic_sum_reactive() {
        // Secs4Net.Tests: number-arithmetic-sum-reactive
        let a = IntegerProperty::new(2);
        let b = IntegerProperty::new(3);
        let sum = a.add(&b);
        assert_eq!(sum.int_value(), 5);
        a.set(10);
        assert_eq!(sum.int_value(), 13);
        b.set(1);
        assert_eq!(sum.int_value(), 11);
    }

    #[test]
    fn number_arithmetic_subtract_negate() {
        // Secs4Net.Tests: number-arithmetic-subtract-negate
        let a = IntegerProperty::new(10);
        let b = IntegerProperty::new(4);
        assert_eq!(a.subtract(&b).int_value(), 6);
        assert_eq!(a.negate().int_value(), -10);
        a.set(7);
        // negate is live
        let neg = a.negate();
        assert_eq!(neg.int_value(), -7);
        a.set(3);
        assert_eq!(neg.int_value(), -3);
    }
}
