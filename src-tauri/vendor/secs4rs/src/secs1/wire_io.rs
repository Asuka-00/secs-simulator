//! SECS-I block frame I/O, ENQ/EOT/ACK handshake, multi-block T4, master/slave.
//!
//! Source: `AbstractSecs1CircuitFacade` send/receive block path (message-level slice).
//! Full async byte-queue circuit / T3 reply manager deferred.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::block::Secs1MessageBlock;
use super::error::{Error, Result};
use super::message::Secs1Message;

/// ENQ — request to send.
pub const ENQ: u8 = 0x05;
/// EOT — ready to receive.
pub const EOT: u8 = 0x04;
/// ACK — block accepted.
pub const ACK: u8 = 0x06;
/// NAK — block rejected.
pub const NAK: u8 = 0x15;

/// Write one complete SECS-I block frame (`length | payload | checksum`).
pub fn write_block<W: Write>(w: &mut W, block: &Secs1MessageBlock) -> Result<()> {
    if !block.is_valid() {
        return Err(Error::InvalidBlocks);
    }
    w.write_all(block.get_bytes())?;
    w.flush()?;
    Ok(())
}

/// Read one complete SECS-I block frame.
///
/// Layout: `len(1)` + `len` bytes (header+body) + checksum(2) = `len + 3` total.
pub fn read_block<R: Read>(r: &mut R) -> Result<Secs1MessageBlock> {
    let mut len_b = [0u8; 1];
    read_exact(r, &mut len_b)?;
    let len = len_b[0] as usize;
    if !(10..=254).contains(&len) {
        return Err(Error::IllegalLengthByte(len as i32));
    }
    let mut rest = vec![0u8; len + 2];
    read_exact(r, &mut rest)?;
    let mut frame = Vec::with_capacity(1 + len + 2);
    frame.push(len_b[0]);
    frame.extend_from_slice(&rest);
    Ok(Secs1MessageBlock::of(frame))
}

fn read_exact<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<()> {
    let mut off = 0;
    while off < buf.len() {
        match r.read(&mut buf[off..]) {
            Ok(0) => return Err(Error::DetectTerminate),
            Ok(n) => off += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn read_one_byte<R: Read>(r: &mut R) -> Result<u8> {
    let mut b = [0u8; 1];
    read_exact(r, &mut b)?;
    Ok(b[0])
}

fn write_one_byte<W: Write>(w: &mut W, b: u8) -> Result<()> {
    w.write_all(&[b])?;
    w.flush()?;
    Ok(())
}

fn map_timeout(err: Error, as_t: Error) -> Error {
    match &err {
        Error::Io(m) if m.starts_with("timeout:") => as_t,
        _ => err,
    }
}

/// Apply read timeout on a `TcpStream`.
pub fn set_read_timeout(stream: &TcpStream, timeout: Option<Duration>) -> Result<()> {
    stream.set_read_timeout(timeout)?;
    Ok(())
}

fn read_one_byte_timeout(stream: &mut TcpStream, t: Duration, as_t: Error) -> Result<u8> {
    set_read_timeout(stream, Some(t))?;
    let b = match read_one_byte(stream) {
        Ok(v) => v,
        Err(e) => {
            let _ = set_read_timeout(stream, None);
            return Err(map_timeout(e, as_t));
        }
    };
    let _ = set_read_timeout(stream, None);
    Ok(b)
}

/// After ENQ is known: EOT → read block → ACK/NAK.
///
/// Used by first-block path and multi-block T4 continuation (ENQ already consumed).
pub fn recv_block_after_enq(stream: &mut TcpStream, t2: Duration) -> Result<Secs1MessageBlock> {
    write_one_byte(stream, EOT)?;

    set_read_timeout(stream, Some(t2))?;
    let block = match read_block(stream) {
        Ok(b) => b,
        Err(e) => {
            let _ = set_read_timeout(stream, None);
            let _ = write_one_byte(stream, NAK);
            return Err(map_timeout(e, Error::TimeoutT2));
        }
    };
    let _ = set_read_timeout(stream, None);

    if block.is_valid() {
        write_one_byte(stream, ACK)?;
        Ok(block)
    } else {
        write_one_byte(stream, NAK)?;
        Err(Error::ChecksumMismatch)
    }
}

/// Receiver path for one block: wait ENQ → EOT → read block → ACK (or NAK if invalid).
pub fn recv_block_handshake(
    stream: &mut TcpStream,
    t2: Duration,
) -> Result<Secs1MessageBlock> {
    let enq = read_one_byte_timeout(stream, t2, Error::TimeoutT2)?;
    if enq != ENQ {
        return Err(Error::Protocol("expected ENQ"));
    }
    recv_block_after_enq(stream, t2)
}

/// Wait for EOT after sending ENQ, with master/slave ENQ contention.
///
/// - Slave (`is_master == false`): peer ENQ → `Error::PeerEnq` (caller yields to receive).
/// - Master: peer ENQ is ignored; keep polling until EOT or T2.
fn wait_eot_after_enq(stream: &mut TcpStream, t2: Duration, is_master: bool) -> Result<()> {
    // Overall T2 budget for the control-character wait (parity: single PollByte(T2)).
    let deadline = std::time::Instant::now() + t2;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(Error::TimeoutT2);
        }
        let b = read_one_byte_timeout(stream, remaining, Error::TimeoutT2)?;
        if b == EOT {
            return Ok(());
        }
        if b == ENQ {
            if !is_master {
                return Err(Error::PeerEnq);
            }
            // Master keeps the line: continue waiting for EOT within remaining T2.
            continue;
        }
        return Err(Error::NotReceiveEot(b));
    }
}

/// Sender path for one block: ENQ → wait EOT → write block → wait ACK.
///
/// `is_master` controls ENQ contention (slave yields with `Error::PeerEnq`).
pub fn send_block_handshake_role(
    stream: &mut TcpStream,
    block: &Secs1MessageBlock,
    t2: Duration,
    is_master: bool,
) -> Result<()> {
    write_one_byte(stream, ENQ)?;
    wait_eot_after_enq(stream, t2, is_master)?;

    write_block(stream, block)?;

    let ack = read_one_byte_timeout(stream, t2, Error::TimeoutT2)?;
    if ack != ACK {
        return Err(Error::NotReceiveAck(ack));
    }
    Ok(())
}

/// Sender path for one block (master role — no yield on peer ENQ).
pub fn send_block_handshake(
    stream: &mut TcpStream,
    block: &Secs1MessageBlock,
    t2: Duration,
) -> Result<()> {
    send_block_handshake_role(stream, block, t2, true)
}

/// Send all blocks of a SECS-I message (each ENQ/EOT/ACK).
///
/// On slave ENQ contention (`PeerEnq`), the caller should receive then retry.
pub fn send_message_role(
    stream: &mut TcpStream,
    msg: &Secs1Message,
    t2: Duration,
    is_master: bool,
) -> Result<()> {
    for block in msg.to_blocks() {
        send_block_handshake_role(stream, block, t2, is_master)?;
    }
    Ok(())
}

/// Send all blocks as master (no ENQ yield).
pub fn send_message(stream: &mut TcpStream, msg: &Secs1Message, t2: Duration) -> Result<()> {
    send_message_role(stream, msg, t2, true)
}

/// Receive a complete multi-block SECS-I message.
///
/// Between non-Ebit blocks, waits for the next ENQ within `t4` (SecsTimeout T4).
pub fn recv_message(
    stream: &mut TcpStream,
    t2: Duration,
    t4: Duration,
) -> Result<Secs1Message> {
    let mut blocks: Vec<Secs1MessageBlock> = Vec::new();
    let first = recv_block_handshake(stream, t2)?;
    let mut done = first.ebit();
    blocks.push(first);

    while !done {
        // Inter-block: next ENQ within T4 (`Receiving` non-Ebit branch).
        let b = read_one_byte_timeout(stream, t4, Error::TimeoutT4)?;
        if b != ENQ {
            return Err(Error::NotReceiveNextBlockEnq(b));
        }
        let next = recv_block_after_enq(stream, t2)?;
        done = next.ebit();
        blocks.push(next);
    }

    Secs1Message::build_from_blocks(&blocks)
}

/// Send with slave yield: if peer ENQs while waiting EOT, receive peer message first,
/// then retry the entire send (cursor reset parity with `pack.Reset()`).
///
/// Returns `Ok(None)` when send completes without receiving a peer message mid-send;
/// `Ok(Some(peer))` when a peer message was received during yield (before own send finished).
///
/// `retry` is max retry count on T2/ACK failure (parity: `Retry`, loop `<= retry`).
pub fn send_message_with_slave_yield(
    stream: &mut TcpStream,
    msg: &Secs1Message,
    t2: Duration,
    t4: Duration,
    is_master: bool,
    retry: u32,
) -> Result<Option<Secs1Message>> {
    let mut peer_msg: Option<Secs1Message> = None;
    let blocks = msg.to_blocks();
    if blocks.is_empty() {
        return Err(Error::EmptyBlockList);
    }

    let mut present = 0usize;
    let mut attempts: u32 = 0;

    while attempts <= retry {
        let block = &blocks[present];
        match send_block_handshake_role(stream, block, t2, is_master) {
            Ok(()) => {
                if block.ebit() {
                    return Ok(peer_msg);
                }
                present += 1;
                attempts = 0;
            }
            Err(Error::PeerEnq) => {
                // Slave yields: receive peer complete message, reset send cursor.
                let peer = recv_message_after_enq(stream, t2, t4)?;
                peer_msg = Some(peer);
                present = 0;
                attempts = 0;
            }
            Err(Error::TimeoutT2) | Err(Error::NotReceiveAck(_)) => {
                attempts += 1;
            }
            Err(e) => return Err(e),
        }
    }
    Err(Error::RetryOver)
}

/// Multi-block receive when the first ENQ has already been consumed (slave yield path).
fn recv_message_after_enq(
    stream: &mut TcpStream,
    t2: Duration,
    t4: Duration,
) -> Result<Secs1Message> {
    let mut blocks: Vec<Secs1MessageBlock> = Vec::new();
    let first = recv_block_after_enq(stream, t2)?;
    let mut done = first.ebit();
    blocks.push(first);

    while !done {
        let b = read_one_byte_timeout(stream, t4, Error::TimeoutT4)?;
        if b != ENQ {
            return Err(Error::NotReceiveNextBlockEnq(b));
        }
        let next = recv_block_after_enq(stream, t2)?;
        done = next.ebit();
        blocks.push(next);
    }

    Secs1Message::build_from_blocks(&blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secs2::Secs2;
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    fn loopback_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = thread::spawn(move || TcpStream::connect(addr).unwrap());
        let (server, _) = listener.accept().unwrap();
        let client = client.join().unwrap();
        (server, client)
    }

    fn header_sys(sys: u8) -> [u8; 10] {
        [
            0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, sys,
        ]
    }

    fn sample_block() -> Secs1MessageBlock {
        let msg =
            Secs1Message::build_data_message(&header_sys(1), Secs2::ascii("AB").unwrap()).unwrap();
        msg.to_blocks()[0].clone()
    }

    fn multi_block_msg() -> Secs1Message {
        // 500 chars → multi-block (244-byte chunks).
        let body = Secs2::ascii("Z".repeat(500)).unwrap();
        Secs1Message::build_data_message(&header_sys(2), body).unwrap()
    }

    #[test]
    fn loopback_write_read_block() {
        let (mut a, mut b) = loopback_pair();
        let block = sample_block();
        assert!(block.is_valid());
        write_block(&mut a, &block).unwrap();
        let got = read_block(&mut b).unwrap();
        assert!(got.is_valid());
        assert_eq!(got.get_bytes(), block.get_bytes());
        assert_eq!(got.block_number(), 1);
        assert!(got.ebit());
    }

    #[test]
    fn loopback_enq_eot_ack_handshake() {
        let (server, client) = loopback_pair();
        let block = sample_block();
        let t2 = Duration::from_secs(2);

        let s_block = block.clone();
        let recv = thread::spawn(move || {
            let mut s = server;
            recv_block_handshake(&mut s, t2).unwrap()
        });

        let mut c = client;
        send_block_handshake(&mut c, &s_block, t2).unwrap();
        let got = recv.join().unwrap();
        assert!(got.is_valid());
        assert_eq!(got.get_bytes(), block.get_bytes());
    }

    #[test]
    fn control_byte_constants() {
        assert_eq!(ENQ, 0x05);
        assert_eq!(EOT, 0x04);
        assert_eq!(ACK, 0x06);
        assert_eq!(NAK, 0x15);
    }

    #[test]
    fn loopback_multi_block_message_roundtrip() {
        let (server, client) = loopback_pair();
        let msg = multi_block_msg();
        assert!(msg.to_blocks().len() >= 2, "need multi-block fixture");
        let t2 = Duration::from_secs(2);
        let t4 = Duration::from_secs(2);
        let expected_ascii = msg.secs2().get_ascii().unwrap();

        let recv = thread::spawn(move || {
            let mut s = server;
            recv_message(&mut s, t2, t4).unwrap()
        });

        let mut c = client;
        send_message(&mut c, &msg, t2).unwrap();
        let got = recv.join().unwrap();
        assert_eq!(got.get_stream(), 1);
        assert_eq!(got.get_function(), 1);
        assert!(got.wbit());
        assert_eq!(got.secs2().get_ascii().unwrap(), expected_ascii);
        assert_eq!(got.to_blocks().len(), msg.to_blocks().len());
    }

    #[test]
    fn recv_message_t4_timeout_after_first_block() {
        // Peer sends only the first (non-Ebit) block then stalls → receiver T4.
        let (server, client) = loopback_pair();
        let msg = multi_block_msg();
        let first = msg.to_blocks()[0].clone();
        assert!(!first.ebit());
        let t2 = Duration::from_secs(2);
        let t4 = Duration::from_millis(200);

        let recv = thread::spawn(move || {
            let mut s = server;
            recv_message(&mut s, t2, t4)
        });

        let mut c = client;
        send_block_handshake(&mut c, &first, t2).unwrap();
        // Do not send further ENQ/blocks.
        let err = recv.join().unwrap().unwrap_err();
        assert_eq!(err, Error::TimeoutT4);
    }

    #[test]
    fn slave_wait_eot_returns_peer_enq() {
        // Both sides write ENQ; slave waiting for EOT must yield with PeerEnq.
        let (mut master, mut slave) = loopback_pair();
        let t2 = Duration::from_millis(500);

        write_one_byte(&mut master, ENQ).unwrap();
        write_one_byte(&mut slave, ENQ).unwrap();

        let err = wait_eot_after_enq(&mut slave, t2, false).unwrap_err();
        assert_eq!(err, Error::PeerEnq);

        // Master as master ignores peer ENQ until T2.
        let err_m = wait_eot_after_enq(&mut master, Duration::from_millis(150), true).unwrap_err();
        assert_eq!(err_m, Error::TimeoutT2);
    }

    #[test]
    fn slave_yield_then_both_messages_exchange() {
        // Master sends; slave also wants to send, yields on ENQ, receives master, then sends;
        // master receives slave after own send completes.
        let (mut master_stream, mut slave_stream) = loopback_pair();
        let t2 = Duration::from_secs(2);
        let t4 = Duration::from_secs(2);

        let master_msg =
            Secs1Message::build_data_message(&header_sys(0x11), Secs2::ascii("M").unwrap())
                .unwrap();
        let slave_msg =
            Secs1Message::build_data_message(&header_sys(0x22), Secs2::ascii("S").unwrap())
                .unwrap();

        let master_send = master_msg.clone();
        let master = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            send_message_role(&mut master_stream, &master_send, t2, true).unwrap();
            recv_message(&mut master_stream, t2, t4).unwrap()
        });

        let slave_send = slave_msg.clone();
        let slave = thread::spawn(move || {
            // Ensure slave ENQ is in flight around the same time as master's.
            let peer = send_message_with_slave_yield(
                &mut slave_stream,
                &slave_send,
                t2,
                t4,
                false,
                5,
            )
            .unwrap();
            peer
        });

        let from_slave = master.join().unwrap();
        let slave_saw = slave.join().unwrap();

        assert_eq!(from_slave.secs2().get_ascii().unwrap(), "S");
        let peer = slave_saw.expect("slave should receive master message while yielding");
        assert_eq!(peer.secs2().get_ascii().unwrap(), "M");
    }
}
