//! List reactive property (oracle Batch3/4 + compute size/contains).
//!
//! Source: `ListProperty` / `AbstractListProperty` / `CollectionObservable`.

use std::time::Duration;

use super::boolean::BooleanProperty;
use super::cell::{ListenerId, Property, WaitTimeout};
use super::integer::IntegerProperty;
use super::timeout::TimeUnit;

/// List property (`ListProperty<E>`).
///
/// Mutation (`add` / `clear` / …) replaces the inner `Vec` so change listeners
/// and waiters observe the same notify semantics as in-place collection mutates.
#[derive(Clone)]
pub struct ListProperty<T>
where
    T: Clone + PartialEq + Send + 'static,
{
    inner: Property<Vec<T>>,
}

impl<T> ListProperty<T>
where
    T: Clone + PartialEq + Send + 'static,
{
    /// `ListProperty.NewInstance()` — empty list.
    pub fn new() -> Self {
        Self {
            inner: Property::new(Vec::new()),
        }
    }

    /// `ListProperty.NewInstance(initial)`.
    pub fn with_initial(initial: impl IntoIterator<Item = T>) -> Self {
        Self {
            inner: Property::new(initial.into_iter().collect()),
        }
    }

    /// Element count (`Count`).
    pub fn count(&self) -> usize {
        self.inner.get().len()
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.inner.get().is_empty()
    }

    /// Snapshot of the list.
    pub fn to_vec(&self) -> Vec<T> {
        self.inner.get()
    }

    /// Element at index (clones).
    pub fn get(&self, index: usize) -> Option<T> {
        self.inner.get().get(index).cloned()
    }

    /// `Contains(item)`.
    pub fn contains(&self, item: &T) -> bool {
        self.inner.get().iter().any(|e| e == item)
    }

    /// `Add(item)` — always appends (List always grows → always notifies).
    pub fn add(&self, item: T) {
        let mut v = self.inner.get();
        v.push(item);
        self.inner.set(v);
    }

    /// `Clear()` — notifies only if was non-empty.
    pub fn clear(&self) {
        if !self.is_empty() {
            self.inner.set(Vec::new());
        }
    }

    /// `Remove(item)` — first match; returns whether removed.
    pub fn remove(&self, item: &T) -> bool {
        let mut v = self.inner.get();
        if let Some(i) = v.iter().position(|e| e == item) {
            v.remove(i);
            self.inner.set(v);
            true
        } else {
            false
        }
    }

    /// `WaitUntilIsNotEmpty()`.
    pub fn wait_until_is_not_empty(&self) {
        self.inner.wait_until(|v| !v.is_empty());
    }

    /// Timed `WaitUntilIsNotEmpty`.
    pub fn wait_until_is_not_empty_timeout(
        &self,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_timeout(|v| !v.is_empty(), unit.to_std_duration(timeout))
    }

    /// Timed wait with `Duration`.
    pub fn wait_until_is_not_empty_duration(
        &self,
        timeout: Duration,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_timeout(|v| !v.is_empty(), timeout)
    }

    /// `WaitUntilIsEmpty()`.
    pub fn wait_until_is_empty(&self) {
        self.inner.wait_until(|v| v.is_empty());
    }

    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&Vec<T>) + Send + Sync + 'static,
    {
        self.inner.add_change_listener(listener)
    }

    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        self.inner.remove_change_listener(id)
    }

    pub fn as_property(&self) -> &Property<Vec<T>> {
        &self.inner
    }

    /// `ComputeSize()` — live integer bound to list length.
    pub fn compute_size(&self) -> IntegerProperty {
        let n = IntegerProperty::new(self.count() as i32);
        let n2 = n.clone();
        self.add_change_listener(move |v| n2.set(v.len() as i32));
        n
    }

    /// `ComputeContains(item)` — live boolean; item is cloned for the bind.
    pub fn compute_contains(&self, item: T) -> BooleanProperty
    where
        T: Sync,
    {
        let bp = BooleanProperty::new(self.contains(&item));
        let bp2 = bp.clone();
        self.add_change_listener(move |v| bp2.set(v.iter().any(|e| e == &item)));
        bp
    }

    /// `ComputeIsEmpty()`.
    pub fn compute_is_empty(&self) -> BooleanProperty {
        let bp = BooleanProperty::new(self.is_empty());
        let bp2 = bp.clone();
        self.add_change_listener(move |v| bp2.set(v.is_empty()));
        bp
    }
}

impl<T> Default for ListProperty<T>
where
    T: Clone + PartialEq + Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn listproperty_ops() {
        // Secs4Net.Tests: listproperty-ops
        let lp = ListProperty::new();
        lp.add("a".to_string());
        lp.add("b".to_string());
        assert_eq!(lp.count(), 2);
        assert_eq!(lp.get(0).as_deref(), Some("a"));
        assert!(lp.contains(&"b".to_string()));
    }

    #[test]
    fn collection_wait_until_is_not_empty_threaded() {
        // Secs4Net.Tests: collection-waitUntilIsNotEmpty-threaded
        let lp = ListProperty::new();
        let lp2 = lp.clone();
        let th = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            lp2.add(1);
        });
        lp.wait_until_is_not_empty();
        assert_eq!(lp.count(), 1);
        assert_eq!(lp.get(0), Some(1));
        th.join().unwrap();
    }

    #[test]
    fn collection_wait_until_is_not_empty_timeout() {
        let lp: ListProperty<i32> = ListProperty::new();
        let r = lp.wait_until_is_not_empty_duration(Duration::from_millis(30));
        assert_eq!(r, Err(WaitTimeout));
    }

    #[test]
    fn list_compute_size_reactive() {
        // Secs4Net.Tests: list-compute-size-reactive
        let lp: ListProperty<i32> = ListProperty::new();
        let size = lp.compute_size();
        assert_eq!(size.int_value(), 0);
        lp.add(7);
        lp.add(8);
        assert_eq!(size.int_value(), 2);
        lp.clear();
        assert_eq!(size.int_value(), 0);
    }

    #[test]
    fn list_compute_contains_reactive() {
        // Secs4Net.Tests: list-compute-contains-reactive
        let lp: ListProperty<i32> = ListProperty::new();
        let has5 = lp.compute_contains(5);
        assert!(!has5.boolean_value());
        lp.add(5);
        assert!(has5.boolean_value());
        lp.remove(&5);
        assert!(!has5.boolean_value());
    }
}
