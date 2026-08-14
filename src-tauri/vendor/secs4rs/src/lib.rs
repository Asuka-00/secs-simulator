//! secs4rs — idiomatic Rust SECS/GEM stack with **result parity** against Secs4Net.
//!
//! Hard constraint: wire bytes, timeouts, state machines, and distinguishable
//! errors match Secs4Net. Internal structure is free to be idiomatic Rust.
//!
//! Authoritative source: `../../Secs4Net/` (C#). Ambiguity: `../../secs4java8/`.

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
// Allow empty modules during scaffolding; tighten as slices land.
#![allow(dead_code, unused_imports)]

pub mod gem;
pub mod hsms;
pub mod hsms_gs;
pub mod hsms_ss;
pub mod open_close;
pub mod property;
pub mod secs1;
pub mod secs1_on_tcp_ip;
pub mod secs2;
pub mod secs_message;
pub mod sml;
pub mod timeout;
pub mod util;

/// Phase 9 integration smokes (GEM session + optional C# interop).
#[cfg(test)]
mod phase9;

pub use open_close::{OpenAndCloseable, OpenCloseError, OpenCloseState};
pub use secs_message::SecsMessage;

/// Zero-dependency test harness (mirrors `Secs4Net.Tests.T`).
/// Available to integration-style unit tests under `#[cfg(test)]`.
#[doc(hidden)]
pub mod t;

/// Crate version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
