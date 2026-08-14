//! Generic reactive cell: get/set, change listeners, wait_until (+ timeout).
//!
//! Idiomatic stand-in for the Secs4Net `AbstractProperty` + wait paths.
//! Uses `Mutex` + `Condvar`; `wait_while` tolerates spurious wakeups while
//! preserving observable results (condition met → Ok; timeout → Err).

use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Error when a timed wait expires before the predicate holds.
///
/// Maps to Java/`System.TimeoutException` on the Secs4Net path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitTimeout;

impl std::fmt::Display for WaitTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "timeout waiting for property condition")
    }
}

impl std::error::Error for WaitTimeout {}

/// Opaque listener id (for remove).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(u64);

impl ListenerId {
    /// Construct from raw id (used by communicator receive-listener registries).
    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

type ListenerFn<T> = Arc<dyn Fn(&T) + Send + Sync + 'static>;

struct State<T> {
    value: T,
    listeners: Vec<(ListenerId, ListenerFn<T>)>,
}

struct Inner<T> {
    state: Mutex<State<T>>,
    cond: Condvar,
    next_id: AtomicU64,
}

/// Thread-safe reactive property cell.
#[derive(Clone)]
pub struct Property<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Property<T>
where
    T: Clone + PartialEq + Send + 'static,
{
    pub fn new(initial: T) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    value: initial,
                    listeners: Vec::new(),
                }),
                cond: Condvar::new(),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    /// Current value (cloned under lock).
    pub fn get(&self) -> T {
        self.inner.state.lock().expect("property lock").value.clone()
    }

    /// Set value; notify listeners and wake waiters only if changed
    /// (`EqualityComparer` / `PartialEq` parity).
    pub fn set(&self, value: T) {
        let (snapshot, listeners) = {
            let mut g = self.inner.state.lock().expect("property lock");
            if g.value == value {
                return;
            }
            g.value = value;
            let snapshot = g.value.clone();
            // Snapshot listeners and drop the lock before callbacks so a listener
            // re-entering get/set/wait_until cannot deadlock on std Mutex.
            let listeners: Vec<ListenerFn<T>> =
                g.listeners.iter().map(|(_, f)| Arc::clone(f)).collect();
            (snapshot, listeners)
        };
        for f in &listeners {
            f(&snapshot);
        }
        self.inner.cond.notify_all();
    }

    /// Add listener; immediately invoked with current value (C# parity).
    /// Returns whether this was a new registration (always true — ids unique).
    pub fn add_change_listener<F>(&self, listener: F) -> ListenerId
    where
        F: Fn(&T) + Send + Sync + 'static,
    {
        let id = ListenerId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let f: ListenerFn<T> = Arc::new(listener);
        let mut g = self.inner.state.lock().expect("property lock");
        let current = g.value.clone();
        g.listeners.push((id, Arc::clone(&f)));
        drop(g);
        // Immediate notify outside lock reduces re-entrancy risk; C# does it under
        // reentrant lock. Observable result: listener sees current value once on add.
        f(&current);
        id
    }

    /// Remove listener by id. Returns true if removed.
    pub fn remove_change_listener(&self, id: ListenerId) -> bool {
        let mut g = self.inner.state.lock().expect("property lock");
        let before = g.listeners.len();
        g.listeners.retain(|(i, _)| *i != id);
        g.listeners.len() != before
    }

    /// Block until `pred(&value)` is true.
    pub fn wait_until<P>(&self, mut pred: P)
    where
        P: FnMut(&T) -> bool,
    {
        let mut g = self.inner.state.lock().expect("property lock");
        while !pred(&g.value) {
            g = self.inner.cond.wait(g).expect("property cond");
        }
    }

    /// Block until predicate holds or timeout elapses.
    pub fn wait_until_timeout<P>(
        &self,
        mut pred: P,
        timeout: Duration,
    ) -> Result<(), WaitTimeout>
    where
        P: FnMut(&T) -> bool,
    {
        if timeout.is_zero() {
            let g = self.inner.state.lock().expect("property lock");
            return if pred(&g.value) {
                Ok(())
            } else {
                Err(WaitTimeout)
            };
        }

        let g = self.inner.state.lock().expect("property lock");
        let (g, wait_result) = self
            .inner
            .cond
            .wait_timeout_while(g, timeout, |s| !pred(&s.value))
            .expect("property cond");

        if pred(&g.value) {
            Ok(())
        } else if wait_result.timed_out() {
            Err(WaitTimeout)
        } else {
            // Spurious / race: re-check already failed — treat as timeout only if
            // still not satisfied (should be rare).
            Err(WaitTimeout)
        }
    }

    /// Wait until value equals `expected` (by `PartialEq`).
    pub fn wait_until_equal_to(&self, expected: &T)
    where
        T: PartialEq,
    {
        self.wait_until(|v| v == expected);
    }

    /// Timed equal wait.
    pub fn wait_until_equal_to_timeout(
        &self,
        expected: &T,
        timeout: Duration,
    ) -> Result<(), WaitTimeout>
    where
        T: PartialEq,
    {
        self.wait_until_timeout(|v| v == expected, timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn set_get_and_listener() {
        let p = Property::new(1);
        let hits = Arc::new(AtomicI32::new(0));
        let h = Arc::clone(&hits);
        p.add_change_listener(move |v| {
            h.fetch_add(*v, Ordering::SeqCst);
        });
        // immediate notify with 1
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        p.set(2);
        assert_eq!(hits.load(Ordering::SeqCst), 1 + 2);
        p.set(2); // no change → no notify
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        assert_eq!(p.get(), 2);
    }

    #[test]
    fn wait_until_already_true() {
        let p = Property::new(true);
        p.wait_until(|v| *v);
    }

    #[test]
    fn wait_until_threaded() {
        let p = Property::new(false);
        let p2 = p.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            p2.set(true);
        });
        p.wait_until(|v| *v);
        assert!(p.get());
    }

    #[test]
    fn set_listener_reenter_set_no_deadlock() {
        let p = Property::new(0i32);
        let p2 = p.clone();
        p.add_change_listener(move |v| {
            if *v == 1 {
                // Re-enter set from listener (must not deadlock).
                p2.set(2);
            }
        });
        p.set(1);
        assert_eq!(p.get(), 2);
    }

    #[test]
    fn wait_until_timeout_fails() {
        let p = Property::new(false);
        let r = p.wait_until_timeout(|v| *v, Duration::from_millis(50));
        assert_eq!(r, Err(WaitTimeout));
    }
}
