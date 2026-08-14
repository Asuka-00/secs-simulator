//! SML message / data-item parsing (minimal oracle coverage).
//!
//! Source behavior: `SmlMessage.Of` for header + optional body.
//! Not a full SML grammar — enough for Batch2 cases.

mod error;
mod parse;

pub use error::{Error, Result};
pub use parse::SmlMessage;
