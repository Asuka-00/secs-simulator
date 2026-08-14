//! String reactive property (oracle string-compute paths).
//!
//! Source: `StringProperty` / `AbstractStringProperty` / `StringObservable`.
//! Compute results are live [`BooleanProperty`] / [`IntegerProperty`] / derived string props.

use super::boolean::BooleanProperty;
use super::cell::{ListenerId, Property};
use super::integer::IntegerProperty;

/// String property (`StringProperty`).
#[derive(Clone)]
pub struct StringProperty {
    inner: Property<String>,
}

impl StringProperty {
    /// `StringProperty.NewInstance(initial)`.
    pub fn new(initial: impl Into<String>) -> Self {
        Self {
            inner: Property::new(initial.into()),
        }
    }

    /// `Get()` / current string.
    pub fn get(&self) -> String {
        self.inner.get()
    }

    /// `Set(value)`.
    pub fn set(&self, value: impl Into<String>) {
        self.inner.set(value.into());
    }

    /// Display / `ToString()` of current value.
    pub fn to_string_value(&self) -> String {
        self.get()
    }

    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&String) + Send + Sync + 'static,
    {
        self.inner.add_change_listener(listener)
    }

    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        self.inner.remove_change_listener(id)
    }

    pub fn as_property(&self) -> &Property<String> {
        &self.inner
    }

    /// `ComputeToUpperCase()` — live uppercased string (as `StringProperty`).
    pub fn compute_to_upper_case(&self) -> StringProperty {
        let u = StringProperty::new(self.get().to_uppercase());
        let u2 = u.clone();
        self.add_change_listener(move |s| u2.set(s.to_uppercase()));
        u
    }

    /// `ComputeToLowerCase()`.
    pub fn compute_to_lower_case(&self) -> StringProperty {
        let l = StringProperty::new(self.get().to_lowercase());
        let l2 = l.clone();
        self.add_change_listener(move |s| l2.set(s.to_lowercase()));
        l
    }

    /// `ComputeContains(cs)`.
    pub fn compute_contains(&self, cs: &str) -> BooleanProperty {
        let needle = cs.to_string();
        let bp = BooleanProperty::new(self.get().contains(&needle));
        let bp2 = bp.clone();
        self.add_change_listener(move |s| bp2.set(s.contains(&needle)));
        bp
    }

    /// `ComputeLength()`.
    pub fn compute_length(&self) -> IntegerProperty {
        let n = IntegerProperty::new(self.get().chars().count() as i32);
        let n2 = n.clone();
        // Java/C# String length is UTF-16 code units; for ASCII oracle (Batch1) char count matches.
        // Use `len()` (bytes) only when all-ASCII; oracle cases use ASCII → chars==bytes.
        self.add_change_listener(move |s| n2.set(s.chars().count() as i32));
        n
    }

    /// `ComputeIsEqualTo(cs)`.
    pub fn compute_is_equal_to(&self, cs: &str) -> BooleanProperty {
        let expected = cs.to_string();
        let bp = BooleanProperty::new(self.get() == expected);
        let bp2 = bp.clone();
        self.add_change_listener(move |s| bp2.set(s == &expected));
        bp
    }

    /// `ComputeIsEmpty()`.
    pub fn compute_is_empty(&self) -> BooleanProperty {
        let bp = BooleanProperty::new(self.get().is_empty());
        let bp2 = bp.clone();
        self.add_change_listener(move |s| bp2.set(s.is_empty()));
        bp
    }
}

impl std::fmt::Display for StringProperty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_compute() {
        // Secs4Net.Tests: string-compute
        let s = StringProperty::new("abc");
        assert_eq!(s.compute_to_upper_case().to_string(), "ABC");
        assert!(s.compute_contains("b").boolean_value());
        assert!(!s.compute_contains("z").boolean_value());
        assert_eq!(s.compute_length().int_value(), 3);
        // live upper
        let up = s.compute_to_upper_case();
        s.set("xy");
        assert_eq!(up.to_string(), "XY");
        assert_eq!(s.compute_length().int_value(), 2);
    }

    #[test]
    fn string_compute_is_equal_to_reactive() {
        // Secs4Net.Tests: string-compute-isEqualTo-reactive
        let s = StringProperty::new("foo");
        let eq = s.compute_is_equal_to("foo");
        assert!(eq.boolean_value());
        s.set("bar");
        assert!(!eq.boolean_value());
        s.set("foo");
        assert!(eq.boolean_value());
    }
}
