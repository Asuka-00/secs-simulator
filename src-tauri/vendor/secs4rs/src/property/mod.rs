//! Minimal reactive property core (protocol-path + oracle compute).
//!
//! ## Covered
//! - [`Property`] cell: get/set, change listeners, `wait_until` (+ timeout)
//! - [`BooleanProperty`] / [`ObjectProperty`] / [`IntegerProperty`] / [`StringProperty`]
//! - [`FloatProperty`] / [`DoubleProperty`]
//! - [`ListProperty`] / [`SetProperty`] / [`MapProperty`]
//! - Compute stand-ins: equal/gt/size/contains/sum/negate/is_null/to_upper…
//! - [`TimeoutAndUnit`] / [`TimeoutProperty`]: float seconds → milliseconds parity
//!
//! ## Deferred
//! Full multi-operand LogicalCompution graph (when protocol needs it).
//!
//! Source: `Secs4Net.Local.Property` (behavior), not 1:1 type graph.

mod boolean;
mod cell;
mod float_double;
mod integer;
mod list;
mod map;
mod object;
mod set;
mod string;
mod timeout;

pub use boolean::BooleanProperty;
pub use cell::{ListenerId, Property, WaitTimeout};
pub use float_double::{DoubleProperty, FloatProperty};
pub use integer::IntegerProperty;
pub use list::ListProperty;
pub use map::MapProperty;
pub use object::ObjectProperty;
pub use set::SetProperty;
pub use string::StringProperty;
pub use timeout::{TimeUnit, TimeoutAndUnit, TimeoutProperty, TimeoutValueError};
