//! SECS message root contract (`SecsMessage`).
//!
//! Source: `Secs4Net.SecsMessage`.

use crate::secs2::Secs2;

/// Minimal SECS message surface shared by HSMS / SECS-I.
pub trait SecsMessage {
    /// Stream number (data messages); control → often -1.
    fn get_stream(&self) -> i32;

    /// Function number (data messages); control → often -1.
    fn get_function(&self) -> i32;

    /// W-bit (reply expected).
    fn wbit(&self) -> bool;

    /// SECS-II body.
    fn secs2(&self) -> &Secs2;

    /// Device-ID (HSMS-SS: same as session).
    fn device_id(&self) -> i32;

    /// Session-ID.
    fn session_id(&self) -> i32;

    /// 10-byte header.
    fn header10_bytes(&self) -> [u8; 10];
}
