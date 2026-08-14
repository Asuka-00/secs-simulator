//! Float / Double reactive properties (protocol-path basics + light compute).
//!
//! Source: `FloatProperty` / `DoubleProperty` / `NumberProperty`.
//! Equality is bit-exact (`==`), matching boxed float/double value equality in the
//! Java/C# property layer for normal finite values used by protocol config.

use std::time::Duration;

use super::boolean::BooleanProperty;
use super::cell::{ListenerId, Property, WaitTimeout};
use super::timeout::TimeUnit;

/// Float property (`FloatProperty`).
#[derive(Clone)]
pub struct FloatProperty {
    inner: Property<f32>,
}

impl FloatProperty {
    /// `FloatProperty.NewInstance(initial)`.
    pub fn new(initial: f32) -> Self {
        Self {
            inner: Property::new(initial),
        }
    }

    /// `FloatValue()` / current value.
    pub fn float_value(&self) -> f32 {
        self.inner.get()
    }

    /// `Set(value)`.
    pub fn set(&self, value: f32) {
        self.inner.set(value);
    }

    pub fn wait_until_equal_to(&self, expected: f32) {
        self.inner.wait_until(|v| *v == expected);
    }

    pub fn wait_until_equal_to_timeout(
        &self,
        expected: f32,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_timeout(|v| *v == expected, unit.to_std_duration(timeout))
    }

    pub fn wait_until_equal_to_duration(
        &self,
        expected: f32,
        timeout: Duration,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_timeout(|v| *v == expected, timeout)
    }

    pub fn wait_until_greater_than(&self, threshold: f32) {
        self.inner.wait_until(|v| *v > threshold);
    }

    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&f32) + Send + Sync + 'static,
    {
        self.inner.add_change_listener(listener)
    }

    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        self.inner.remove_change_listener(id)
    }

    pub fn as_property(&self) -> &Property<f32> {
        &self.inner
    }

    /// `ComputeIsEqualTo(n)`.
    pub fn compute_is_equal_to(&self, n: f32) -> BooleanProperty {
        let bp = BooleanProperty::new(self.float_value() == n);
        let bp2 = bp.clone();
        self.add_change_listener(move |v| bp2.set(*v == n));
        bp
    }

    /// `ComputeIsGreaterThan(n)`.
    pub fn compute_is_greater_than(&self, n: f32) -> BooleanProperty {
        let bp = BooleanProperty::new(self.float_value() > n);
        let bp2 = bp.clone();
        self.add_change_listener(move |v| bp2.set(*v > n));
        bp
    }

    /// `Add(other)` — live sum.
    pub fn add(&self, other: &FloatProperty) -> FloatProperty {
        let sum = FloatProperty::new(self.float_value() + other.float_value());
        let sum_a = sum.clone();
        let other_a = other.clone();
        self.add_change_listener(move |a| sum_a.set(*a + other_a.float_value()));
        let sum_b = sum.clone();
        let self_b = self.clone();
        other.add_change_listener(move |b| sum_b.set(self_b.float_value() + *b));
        sum
    }
}

/// Double property (`DoubleProperty`).
#[derive(Clone)]
pub struct DoubleProperty {
    inner: Property<f64>,
}

impl DoubleProperty {
    /// `DoubleProperty.NewInstance(initial)`.
    pub fn new(initial: f64) -> Self {
        Self {
            inner: Property::new(initial),
        }
    }

    /// `DoubleValue()` / current value.
    pub fn double_value(&self) -> f64 {
        self.inner.get()
    }

    /// `Set(value)`.
    pub fn set(&self, value: f64) {
        self.inner.set(value);
    }

    pub fn wait_until_equal_to(&self, expected: f64) {
        self.inner.wait_until(|v| *v == expected);
    }

    pub fn wait_until_equal_to_timeout(
        &self,
        expected: f64,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_timeout(|v| *v == expected, unit.to_std_duration(timeout))
    }

    pub fn wait_until_greater_than(&self, threshold: f64) {
        self.inner.wait_until(|v| *v > threshold);
    }

    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&f64) + Send + Sync + 'static,
    {
        self.inner.add_change_listener(listener)
    }

    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        self.inner.remove_change_listener(id)
    }

    pub fn as_property(&self) -> &Property<f64> {
        &self.inner
    }

    /// `ComputeIsEqualTo(n)`.
    pub fn compute_is_equal_to(&self, n: f64) -> BooleanProperty {
        let bp = BooleanProperty::new(self.double_value() == n);
        let bp2 = bp.clone();
        self.add_change_listener(move |v| bp2.set(*v == n));
        bp
    }

    /// `ComputeIsGreaterThan(n)`.
    pub fn compute_is_greater_than(&self, n: f64) -> BooleanProperty {
        let bp = BooleanProperty::new(self.double_value() > n);
        let bp2 = bp.clone();
        self.add_change_listener(move |v| bp2.set(*v > n));
        bp
    }

    /// `Add(other)` — live sum.
    pub fn add(&self, other: &DoubleProperty) -> DoubleProperty {
        let sum = DoubleProperty::new(self.double_value() + other.double_value());
        let sum_a = sum.clone();
        let other_a = other.clone();
        self.add_change_listener(move |a| sum_a.set(*a + other_a.double_value()));
        let sum_b = sum.clone();
        let self_b = self.clone();
        other.add_change_listener(move |b| sum_b.set(self_b.double_value() + *b));
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn float_property_basic_and_compute() {
        let p = FloatProperty::new(1.5);
        assert_eq!(p.float_value(), 1.5);
        p.set(3.25);
        assert_eq!(p.float_value(), 3.25);
        let eq = p.compute_is_equal_to(3.25);
        assert!(eq.boolean_value());
        p.set(0.0);
        assert!(!eq.boolean_value());
        let gt = p.compute_is_greater_than(-1.0);
        assert!(gt.boolean_value());
    }

    #[test]
    fn float_add_reactive() {
        let a = FloatProperty::new(1.0);
        let b = FloatProperty::new(2.5);
        let sum = a.add(&b);
        assert_eq!(sum.float_value(), 3.5);
        a.set(10.0);
        assert_eq!(sum.float_value(), 12.5);
    }

    #[test]
    fn float_wait_until_equal_threaded() {
        let p = FloatProperty::new(0.0);
        let p2 = p.clone();
        let th = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            p2.set(9.0);
        });
        p.wait_until_equal_to(9.0);
        assert_eq!(p.float_value(), 9.0);
        th.join().unwrap();
    }

    #[test]
    fn double_property_basic_and_compute() {
        let p = DoubleProperty::new(std::f64::consts::PI);
        assert_eq!(p.double_value(), std::f64::consts::PI);
        p.set(2.0);
        let eq = p.compute_is_equal_to(2.0);
        assert!(eq.boolean_value());
        let a = DoubleProperty::new(1.25);
        let b = DoubleProperty::new(0.75);
        assert_eq!(a.add(&b).double_value(), 2.0);
    }
}
