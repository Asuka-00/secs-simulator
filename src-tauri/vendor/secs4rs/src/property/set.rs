//! Set reactive property (oracle Batch3 path).
//!
//! Source: `SetProperty` / `AbstractSetProperty` / `AbstractCollectionProperty`.
//! `Add` returns whether the set changed (ISet semantics); duplicates do not notify.

use std::collections::HashSet;
use std::hash::Hash;
use std::time::Duration;

use super::cell::{ListenerId, Property, WaitTimeout};
use super::timeout::TimeUnit;

/// Set property (`SetProperty<E>`).
#[derive(Clone)]
pub struct SetProperty<T>
where
    T: Clone + Eq + Hash + Send + 'static,
{
    inner: Property<HashSet<T>>,
}

impl<T> SetProperty<T>
where
    T: Clone + Eq + Hash + Send + 'static,
{
    /// `SetProperty.NewInstance()` — empty set.
    pub fn new() -> Self {
        Self {
            inner: Property::new(HashSet::new()),
        }
    }

    /// `SetProperty.NewInstance(initial)`.
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

    /// Snapshot of the set.
    pub fn to_set(&self) -> HashSet<T> {
        self.inner.get()
    }

    /// `Contains(item)`.
    pub fn contains(&self, item: &T) -> bool {
        self.inner.get().contains(item)
    }

    /// `ISet.Add(item)` — returns whether the set changed (duplicate → false, no notify).
    pub fn add(&self, item: T) -> bool {
        let mut s = self.inner.get();
        if s.insert(item) {
            self.inner.set(s);
            true
        } else {
            false
        }
    }

    /// `Remove(item)` — returns whether removed.
    pub fn remove(&self, item: &T) -> bool {
        let mut s = self.inner.get();
        if s.remove(item) {
            self.inner.set(s);
            true
        } else {
            false
        }
    }

    /// `Clear()` — notifies only if was non-empty.
    pub fn clear(&self) {
        if !self.is_empty() {
            self.inner.set(HashSet::new());
        }
    }

    /// `WaitUntilIsNotEmpty()`.
    pub fn wait_until_is_not_empty(&self) {
        self.inner.wait_until(|s| !s.is_empty());
    }

    /// Timed `WaitUntilIsNotEmpty`.
    pub fn wait_until_is_not_empty_timeout(
        &self,
        timeout: i64,
        unit: TimeUnit,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_timeout(|s| !s.is_empty(), unit.to_std_duration(timeout))
    }

    pub fn wait_until_is_not_empty_duration(
        &self,
        timeout: Duration,
    ) -> Result<(), WaitTimeout> {
        self.inner
            .wait_until_timeout(|s| !s.is_empty(), timeout)
    }

    /// `WaitUntilIsEmpty()`.
    pub fn wait_until_is_empty(&self) {
        self.inner.wait_until(|s| s.is_empty());
    }

    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&HashSet<T>) + Send + Sync + 'static,
    {
        self.inner.add_change_listener(listener)
    }

    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        self.inner.remove_change_listener(id)
    }

    pub fn as_property(&self) -> &Property<HashSet<T>> {
        &self.inner
    }
}

impl<T> Default for SetProperty<T>
where
    T: Clone + Eq + Hash + Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn setproperty_add_remove() {
        // Secs4Net.Tests: setproperty-add-remove
        let sp = SetProperty::new();
        assert!(sp.add(5));
        assert!(!sp.add(5)); // duplicate → false
        assert!(sp.contains(&5));
        assert!(sp.remove(&5));
        assert_eq!(sp.count(), 0);
    }

    #[test]
    fn setproperty_add_duplicate_no_extra_notify() {
        let sp = SetProperty::new();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        sp.add_change_listener(move |_| {
            h.fetch_add(1, Ordering::SeqCst);
        });
        // immediate notify on add_change_listener
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(sp.add(1));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        assert!(!sp.add(1));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn setproperty_wait_until_is_not_empty_threaded() {
        let sp = SetProperty::new();
        let sp2 = sp.clone();
        let th = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            sp2.add(7);
        });
        sp.wait_until_is_not_empty();
        assert!(sp.contains(&7));
        th.join().unwrap();
    }
}
