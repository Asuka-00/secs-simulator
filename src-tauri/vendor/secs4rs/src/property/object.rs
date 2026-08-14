//! Object (generic) reactive property.
//!
//! Source: `ObjectProperty<T>` / `AbstractObjectProperty<T>` —
//! get/set + wait-until-equal (constant ref path used by protocol/tests).
//! `ComputeIsNull` for nullable refs → `ObjectProperty<Option<T>>`.

use std::time::Duration;

use super::boolean::BooleanProperty;
use super::cell::{ListenerId, Property, WaitTimeout};
use super::timeout::TimeUnit;

/// Object property (`ObjectProperty<T>`).
///
/// For nullable refs from Java/C#, use `ObjectProperty<Option<T>>`.
#[derive(Clone)]
pub struct ObjectProperty<T> {
    inner: Property<T>,
}

impl<T> ObjectProperty<T>
where
    T: Clone + PartialEq + Send + 'static,
{
    /// `ObjectProperty.NewInstance(initial)`.
    pub fn new(initial: T) -> Self {
        Self {
            inner: Property::new(initial),
        }
    }

    /// `Get()`.
    pub fn get(&self) -> T {
        self.inner.get()
    }

    /// `Set(value)`.
    pub fn set(&self, value: T) {
        self.inner.set(value);
    }

    /// `WaitUntilEqualTo(ref)` — constant-ref path.
    pub fn wait_until_equal_to(&self, expected: &T) {
        self.inner.wait_until_equal_to(expected);
    }

    /// Timed equal wait (`timeout` + `TimeUnit`).
    pub fn wait_until_equal_to_timeout(
        &self,
        expected: &T,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_equal_to_timeout(expected, unit.to_std_duration(timeout))
    }

    /// Duration convenience.
    pub fn wait_until_equal_to_duration(
        &self,
        expected: &T,
        timeout: Duration,
    ) -> Result<(), WaitTimeout> {
        self.inner.wait_until_equal_to_timeout(expected, timeout)
    }

    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&T) + Send + Sync + 'static,
    {
        self.inner.add_change_listener(listener)
    }

    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        self.inner.remove_change_listener(id)
    }

    pub fn as_property(&self) -> &Property<T> {
        &self.inner
    }
}

impl<T> ObjectProperty<Option<T>>
where
    T: Clone + PartialEq + Send + 'static,
{
    /// `IsNull()` — true when value is `None`.
    pub fn is_null(&self) -> bool {
        self.get().is_none()
    }

    /// `ComputeIsNull()` — live `BooleanProperty` bound to nullness.
    ///
    /// Idiomatic stand-in for `BooleanCompution`: updates when the object changes.
    pub fn compute_is_null(&self) -> BooleanProperty {
        let bp = BooleanProperty::new(self.is_null());
        let bp2 = bp.clone();
        self.add_change_listener(move |v| {
            bp2.set(v.is_none());
        });
        bp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn object_wait_until_equal_to_threaded() {
        // Secs4Net.Tests: object-waitUntilEqualTo-threaded
        let op = ObjectProperty::new(String::from("init"));
        let op2 = op.clone();
        let th = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            op2.set(String::from("ready"));
        });
        op.wait_until_equal_to(&String::from("ready"));
        assert_eq!(op.get(), "ready");
        th.join().unwrap();
    }

    #[test]
    fn object_compute_isnull() {
        // Secs4Net.Tests: object-compute-isnull
        // C# null → Option::None
        let op: ObjectProperty<Option<String>> = ObjectProperty::new(None);
        let is_null = op.compute_is_null();
        assert!(is_null.boolean_value());
        op.set(Some(String::from("x")));
        assert!(!is_null.boolean_value());
        op.set(None);
        assert!(is_null.boolean_value());
    }
}
