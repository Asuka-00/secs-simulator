//! SECS-I data message builder (device / system-bytes / R-bit / W-bit).
//!
//! Source: `AbstractSecs1MessageBuilder` + `AbstractSecsMessageBuilder.System4Bytes`.

use crate::hsms::SystemBytesCounter;
use crate::secs2::Secs2;

use super::error::{Error, Result};
use super::message::Secs1Message;

/// Illegal Device-ID (must be 0..=0x7FFF).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceIdIllegalArgument(pub i32);

impl std::fmt::Display for DeviceIdIllegalArgument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Device-ID is in 0 - 32767, id={}", self.0)
    }
}

impl std::error::Error for DeviceIdIllegalArgument {}

/// Validate Device-ID range (`DeviceIdIllegalArgumentException`).
pub fn check_device_id(device_id: i32) -> std::result::Result<(), DeviceIdIllegalArgument> {
    if !(0..=0x7FFF).contains(&device_id) {
        return Err(DeviceIdIllegalArgument(device_id));
    }
    Ok(())
}

/// Encode Device-ID to 2 header device bytes (high 7 bits of first byte).
pub fn device2_bytes(device_id: i32) -> [u8; 2] {
    [
        ((device_id >> 8) & 0x7F) as u8,
        (device_id & 0xFF) as u8,
    ]
}

/// Build primary SECS-I data message (`BuildDataMessage(comm, strm, func, wbit, body)`).
///
/// - Equipment sets R-bit on header[0]
/// - `wbit` sets high bit of stream byte
/// - system-bytes from `SystemBytesCounter` (equip high-2 = device-id)
pub fn build_primary(
    device_id: i32,
    is_equip: bool,
    sys: &SystemBytesCounter,
    strm: i32,
    func: i32,
    wbit: bool,
    body: Secs2,
) -> Result<Secs1Message> {
    let dd = device2_bytes(device_id);
    // SECS-I equip system-bytes high 2 = device id (same packing as HSMS equip session).
    let ssss = sys.next(is_equip, device_id);
    let mut header = [
        dd[0],
        dd[1],
        (strm & 0x7F) as u8,
        func as u8,
        0,
        0,
        ssss[0],
        ssss[1],
        ssss[2],
        ssss[3],
    ];
    if is_equip {
        header[0] |= 0x80;
    }
    if wbit {
        header[2] |= 0x80;
    }
    Secs1Message::build_data_message(&header, body)
}

/// Build reply reusing primary system-bytes (`BuildDataMessage(comm, primary, ...)`).
pub fn build_reply(
    device_id: i32,
    is_equip: bool,
    primary: &Secs1Message,
    strm: i32,
    func: i32,
    wbit: bool,
    body: Secs2,
) -> Result<Secs1Message> {
    let dd = device2_bytes(device_id);
    let pp = primary.header10_bytes();
    let mut header = [
        dd[0],
        dd[1],
        (strm & 0x7F) as u8,
        func as u8,
        0,
        0,
        pp[6],
        pp[7],
        pp[8],
        pp[9],
    ];
    if is_equip {
        header[0] |= 0x80;
    }
    if wbit {
        header[2] |= 0x80;
    }
    Secs1Message::build_data_message(&header, body)
}

/// Convenience: empty body primary.
pub fn build_primary_empty(
    device_id: i32,
    is_equip: bool,
    sys: &SystemBytesCounter,
    strm: i32,
    func: i32,
    wbit: bool,
) -> Result<Secs1Message> {
    build_primary(device_id, is_equip, sys, strm, func, wbit, Secs2::empty())
}

/// Header-only empty body (`BuildDataMessage(header)`).
pub fn build_from_header(header: &[u8]) -> Result<Secs1Message> {
    if header.len() != 10 {
        return Err(Error::HeaderByteLength);
    }
    Secs1Message::build_data_message(header, Secs2::empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device2_bytes_and_range() {
        assert_eq!(device2_bytes(10), [0x00, 0x0A]);
        assert_eq!(device2_bytes(0x7FFF), [0x7F, 0xFF]);
        assert!(check_device_id(0).is_ok());
        assert!(check_device_id(0x7FFF).is_ok());
        assert!(check_device_id(-1).is_err());
        assert!(check_device_id(0x8000).is_err());
    }

    #[test]
    fn build_primary_host_wbit() {
        let sys = SystemBytesCounter::new();
        let m = build_primary(10, false, &sys, 1, 1, true, Secs2::ascii("X").unwrap()).unwrap();
        assert_eq!(m.device_id(), 10);
        assert!(!m.rbit());
        assert!(m.wbit());
        assert_eq!(m.get_stream(), 1);
        assert_eq!(m.get_function(), 1);
        let h = m.header10_bytes();
        // Host system-bytes high 2 zero; serial starts at 1.
        assert_eq!(&h[6..10], &[0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn build_primary_equip_rbit_sys() {
        let sys = SystemBytesCounter::new();
        let m = build_primary(0x12, true, &sys, 2, 3, false, Secs2::empty()).unwrap();
        assert!(m.rbit());
        assert!(!m.wbit());
        assert_eq!(m.device_id(), 0x12);
        let h = m.header10_bytes();
        assert_eq!(h[0] & 0x80, 0x80);
        // Equip system-bytes high 2 = device id.
        assert_eq!(&h[6..8], &[0x00, 0x12]);
        assert_eq!(&h[8..10], &[0x00, 0x01]);
    }

    #[test]
    fn build_reply_reuses_system_bytes() {
        let sys = SystemBytesCounter::new();
        let primary =
            build_primary(10, false, &sys, 1, 1, true, Secs2::ascii("Q").unwrap()).unwrap();
        let reply =
            build_reply(10, false, &primary, 1, 2, false, Secs2::ascii("A").unwrap()).unwrap();
        assert_eq!(&reply.header10_bytes()[6..10], &primary.header10_bytes()[6..10]);
        assert_eq!(reply.get_function(), 2);
        assert!(!reply.wbit());
        assert_eq!(reply.secs2().get_ascii().unwrap(), "A");
    }

    #[test]
    fn system_bytes_auto_increment() {
        let sys = SystemBytesCounter::new();
        let m1 = build_primary_empty(1, false, &sys, 1, 1, false).unwrap();
        let m2 = build_primary_empty(1, false, &sys, 1, 1, false).unwrap();
        assert_eq!(&m1.header10_bytes()[8..10], &[0x00, 0x01]);
        assert_eq!(&m2.header10_bytes()[8..10], &[0x00, 0x02]);
    }
}
