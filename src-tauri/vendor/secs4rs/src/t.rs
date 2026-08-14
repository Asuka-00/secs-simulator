//! Zero-dependency test helpers mirroring `Secs4Net.Tests.T`.
//!
//! Prefer calling these from `#[test]` functions. A sequential runner
//! (`case` + `run`) is also provided for oracle-style batch suites.

use std::fmt::Debug;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

type CaseFn = Box<dyn FnOnce() + Send>;

static CASES: Mutex<Vec<(String, Option<CaseFn>)>> = Mutex::new(Vec::new());

/// Register a named case (for batch runners). Prefer `#[test]` for new code.
pub fn case<F>(name: impl Into<String>, body: F)
where
    F: FnOnce() + Send + 'static,
{
    CASES
        .lock()
        .expect("cases lock")
        .push((name.into(), Some(Box::new(body))));
}

/// Run all registered cases; returns `(pass, fail)` counts.
/// Prints `[PASS]` / `[FAIL]` lines like the C# harness.
pub fn run() -> (usize, usize) {
    let mut cases = CASES.lock().expect("cases lock");
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for (name, slot) in cases.iter_mut() {
        let Some(body) = slot.take() else {
            continue;
        };
        match catch_unwind(AssertUnwindSafe(body)) {
            Ok(()) => {
                pass += 1;
                println!("  [PASS] {name}");
            }
            Err(payload) => {
                fail += 1;
                let msg = panic_message(&payload);
                failures.push(format!("{name}: {msg}"));
                println!("  [FAIL] {name} -> {msg}");
            }
        }
    }
    cases.clear();

    let total = pass + fail;
    println!();
    println!("==== 测试结果:{pass}/{total} 通过,{fail} 失败 ====");
    if !failures.is_empty() {
        println!("---- 失败明细 ----");
        for f in &failures {
            println!("  {f}");
        }
    }
    (pass, fail)
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "box<Any> panic".to_string()
    }
}

/// Assert condition is true.
#[inline]
pub fn assert_true(cond: bool, msg: &str) {
    if !cond {
        panic!("{msg}");
    }
}

/// Assert equality (Debug for messages).
#[inline]
pub fn assert_eq<T: PartialEq + Debug>(expected: T, actual: T, msg: &str) {
    if expected != actual {
        panic!("expected=[{expected:?}] actual=[{actual:?}] {msg}");
    }
}

/// Assert that `body` panics or returns `Err` matching the predicate.
/// For `Result`-style APIs prefer asserting on `Err` variants directly.
#[inline]
pub fn assert_panics<F: FnOnce()>(body: F, msg: &str) {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(()) => panic!("expected panic but completed ok {msg}"),
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_assert_eq_ok() {
        assert_eq(1, 1, "ints");
        assert_true(true, "t");
    }

    #[test]
    #[should_panic(expected = "expected=[1] actual=[2]")]
    fn harness_assert_eq_fail() {
        assert_eq(1, 2, "ints");
    }

    #[test]
    fn case_runner_pass() {
        case("smoke-pass", || {
            assert_eq(2 + 2, 4, "add");
        });
        let (pass, fail) = run();
        assert_eq(1, pass, "pass");
        assert_eq(0, fail, "fail");
    }
}
