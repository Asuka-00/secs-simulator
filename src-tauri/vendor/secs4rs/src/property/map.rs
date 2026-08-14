//! Map reactive property (oracle map-compute-containsKey path).
//!
//! Source: `MapProperty` / `AbstractMapProperty` / `MapObservable`.

use std::collections::HashMap;
use std::hash::Hash;

use super::boolean::BooleanProperty;
use super::cell::{ListenerId, Property};
use super::integer::IntegerProperty;

/// Map property (`MapProperty<K,V>`).
#[derive(Clone)]
pub struct MapProperty<K, V>
where
    K: Clone + Eq + Hash + Send + 'static,
    V: Clone + PartialEq + Send + 'static,
{
    inner: Property<HashMap<K, V>>,
}

impl<K, V> MapProperty<K, V>
where
    K: Clone + Eq + Hash + Send + 'static,
    V: Clone + PartialEq + Send + 'static,
{
    /// `MapProperty.NewInstance()` — empty map.
    pub fn new() -> Self {
        Self {
            inner: Property::new(HashMap::new()),
        }
    }

    /// Element count (`Count`).
    pub fn count(&self) -> usize {
        self.inner.get().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.get().is_empty()
    }

    /// Snapshot.
    pub fn to_map(&self) -> HashMap<K, V> {
        self.inner.get()
    }

    /// `ContainsKey(key)`.
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner.get().contains_key(key)
    }

    /// `TryGetValue` / get clone.
    pub fn get(&self, key: &K) -> Option<V> {
        self.inner.get().get(key).cloned()
    }

    /// Indexer set / `Add`/`put` — always notifies when value changes or key is new.
    pub fn insert(&self, key: K, value: V) {
        let mut m = self.inner.get();
        let changed = match m.get(&key) {
            Some(old) => old != &value,
            None => true,
        };
        if changed {
            m.insert(key, value);
            self.inner.set(m);
        }
    }

    /// `Remove(key)` — returns whether removed.
    pub fn remove(&self, key: &K) -> bool {
        let mut m = self.inner.get();
        if m.remove(key).is_some() {
            self.inner.set(m);
            true
        } else {
            false
        }
    }

    /// `Clear()`.
    pub fn clear(&self) {
        if !self.is_empty() {
            self.inner.set(HashMap::new());
        }
    }

    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&HashMap<K, V>) + Send + Sync + 'static,
    {
        self.inner.add_change_listener(listener)
    }

    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        self.inner.remove_change_listener(id)
    }

    /// `ComputeContainsKey(key)`.
    pub fn compute_contains_key(&self, key: K) -> BooleanProperty
    where
        K: Sync,
    {
        let bp = BooleanProperty::new(self.contains_key(&key));
        let bp2 = bp.clone();
        self.add_change_listener(move |m| bp2.set(m.contains_key(&key)));
        bp
    }

    /// `ComputeSize()`.
    pub fn compute_size(&self) -> IntegerProperty {
        let n = IntegerProperty::new(self.count() as i32);
        let n2 = n.clone();
        self.add_change_listener(move |m| n2.set(m.len() as i32));
        n
    }

    /// `ComputeIsEmpty()`.
    pub fn compute_is_empty(&self) -> BooleanProperty {
        let bp = BooleanProperty::new(self.is_empty());
        let bp2 = bp.clone();
        self.add_change_listener(move |m| bp2.set(m.is_empty()));
        bp
    }
}

impl<K, V> Default for MapProperty<K, V>
where
    K: Clone + Eq + Hash + Send + 'static,
    V: Clone + PartialEq + Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_compute_contains_key_reactive() {
        // Secs4Net.Tests: map-compute-containsKey-reactive
        let mp: MapProperty<String, i32> = MapProperty::new();
        let has_k = mp.compute_contains_key("k".into());
        assert!(!has_k.boolean_value());
        mp.insert("k".into(), 1);
        assert!(has_k.boolean_value());
        mp.remove(&"k".into());
        assert!(!has_k.boolean_value());
    }

    #[test]
    fn map_insert_get_size() {
        let mp: MapProperty<String, i32> = MapProperty::new();
        assert!(mp.is_empty());
        mp.insert("a".into(), 1);
        mp.insert("b".into(), 2);
        assert_eq!(mp.count(), 2);
        assert_eq!(mp.get(&"a".into()), Some(1));
        let size = mp.compute_size();
        assert_eq!(size.int_value(), 2);
        mp.insert("a".into(), 1); // no change → no notify (size still 2)
        assert_eq!(size.int_value(), 2);
        mp.insert("a".into(), 9);
        assert_eq!(mp.get(&"a".into()), Some(9));
    }
}
