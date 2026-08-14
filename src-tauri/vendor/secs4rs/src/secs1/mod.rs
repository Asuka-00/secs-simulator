//! SECS-I blocks, checksum, multi-block assembly, block/message I/O, circuit.
//!
//! Source: `Secs4Net.Secs1` (message / block / builder / circuit).

mod block;
mod builder;
mod circuit;
mod error;
mod message;
mod wire_io;

pub use block::Secs1MessageBlock;
pub use builder::{
    build_from_header, build_primary, build_primary_empty, build_reply, check_device_id,
    device2_bytes, DeviceIdIllegalArgument,
};
pub use circuit::{Secs1Circuit, Secs1CircuitConfig};
pub use error::{Error, Result};
pub use message::Secs1Message;
pub use wire_io::{
    read_block, recv_block_after_enq, recv_block_handshake, recv_message, send_block_handshake,
    send_block_handshake_role, send_message, send_message_role, send_message_with_slave_yield,
    set_read_timeout, write_block, ACK, ENQ, EOT, NAK,
};
