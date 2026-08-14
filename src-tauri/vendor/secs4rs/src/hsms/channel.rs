//! Blocking HSMS TCP frame I/O (send/receive one complete message).
//!
//! Source behavior: `AbstractHsmsAsynchronousSocketChannelFacade` write loop +
//! receive length/header/body assembly — idiomatic blocking `Read`/`Write`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::error::{io_timeout_as_t8, Error, Result};
use super::message::HsmsMessage;
use super::wire::{decode_frame, encode_frame};

/// Write one complete HSMS frame (handles partial writes).
pub fn write_frame<W: Write>(w: &mut W, msg: &HsmsMessage) -> Result<()> {
    let bytes = encode_frame(msg)?;
    w.write_all(&bytes)?;
    w.flush()?;
    Ok(())
}

/// Read one complete HSMS frame (handles partial reads).
///
/// On peer close during read → `Error::DetectTerminate`.
pub fn read_frame<R: Read>(r: &mut R) -> Result<HsmsMessage> {
    let mut len_buf = [0u8; 4];
    read_exact(r, &mut len_buf)?;
    let msg_len = u32::from_be_bytes(len_buf) as usize;
    if msg_len < 10 {
        return Err(Error::LengthBytesLowerThanTen);
    }
    let mut rest = vec![0u8; msg_len];
    read_exact(r, &mut rest)?;
    let mut frame = Vec::with_capacity(4 + msg_len);
    frame.extend_from_slice(&len_buf);
    frame.extend_from_slice(&rest);
    decode_frame(&frame).map(|(m, _)| m)
}

/// Live-session frame read with correct SEMI T8 semantics.
///
/// - **Between messages** (waiting for first length byte): no idle timeout —
///   a quiet Selected link must stay up indefinitely (use Linktest for keepalive).
/// - **Within a frame** (after the first byte of length): each further read is
///   bounded by `t8` (inter-character / incomplete-frame). Timeout → `TimeoutT8`.
///
/// Previously applying socket `read_timeout=T8` to the whole receive loop made
/// idle links drop after T8 and then Active T5-retry / Passive rebind — false churn.
pub fn read_frame_t8(stream: &mut TcpStream, t8: Duration) -> Result<HsmsMessage> {
    // Phase 1: block until first length byte (or peer close / local shutdown).
    set_read_timeout(stream, None)?;
    let mut len_buf = [0u8; 4];
    read_exact_one_byte(stream, &mut len_buf[0])?;

    // Phase 2: remainder of frame under T8.
    set_read_timeout(stream, Some(t8))?;
    let assembled = (|| -> Result<HsmsMessage> {
        read_exact(stream, &mut len_buf[1..4])?;
        let msg_len = u32::from_be_bytes(len_buf) as usize;
        if msg_len < 10 {
            return Err(Error::LengthBytesLowerThanTen);
        }
        let mut rest = vec![0u8; msg_len];
        read_exact(stream, &mut rest)?;
        let mut frame = Vec::with_capacity(4 + msg_len);
        frame.extend_from_slice(&len_buf);
        frame.extend_from_slice(&rest);
        decode_frame(&frame).map(|(m, _)| m)
    })();

    // Clear timeout for next idle wait (best-effort).
    let _ = set_read_timeout(stream, None);
    assembled.map_err(io_timeout_as_t8)
}

fn read_exact_one_byte(r: &mut impl Read, byte: &mut u8) -> Result<()> {
    let mut buf = [0u8; 1];
    loop {
        match r.read(&mut buf) {
            Ok(0) => return Err(Error::DetectTerminate),
            Ok(1) => {
                *byte = buf[0];
                return Ok(());
            }
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
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

/// Apply read timeout on a `TcpStream` (T8-style inter-byte/read deadline).
pub fn set_read_timeout(stream: &TcpStream, timeout: Option<Duration>) -> Result<()> {
    stream.set_read_timeout(timeout)?;
    Ok(())
}

/// Apply write timeout.
pub fn set_write_timeout(stream: &TcpStream, timeout: Option<Duration>) -> Result<()> {
    stream.set_write_timeout(timeout)?;
    Ok(())
}

/// Thin helper bundling a connected stream for frame I/O.
pub struct HsmsTcpChannel {
    stream: TcpStream,
}

impl HsmsTcpChannel {
    pub fn new(stream: TcpStream) -> Self {
        Self { stream }
    }

    pub fn into_inner(self) -> TcpStream {
        self.stream
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<()> {
        set_read_timeout(&self.stream, timeout)
    }

    pub fn write_message(&mut self, msg: &HsmsMessage) -> Result<()> {
        write_frame(&mut self.stream, msg)
    }

    pub fn read_message(&mut self) -> Result<HsmsMessage> {
        read_frame(&mut self.stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::builder::build_select_response;
    use crate::hsms::message_type::HsmsMessageType;
    use crate::hsms::status::SelectStatus;
    use crate::secs2::Secs2;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn loopback_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = thread::spawn(move || TcpStream::connect(addr).unwrap());
        let (server, _) = listener.accept().unwrap();
        let client = client.join().unwrap();
        (server, client)
    }

    #[test]
    fn loopback_select_req_rsp() {
        let (mut server, mut client) = loopback_pair();

        // Client sends SELECT_REQ
        let req_h = [0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01, 0xAA, 0xBB, 0xCC, 0xDD];
        let req = HsmsMessage::of(&req_h).unwrap();
        write_frame(&mut client, &req).unwrap();

        let got = read_frame(&mut server).unwrap();
        assert_eq!(got.message_type(), HsmsMessageType::SelectReq);

        let rsp = build_select_response(&got, SelectStatus::Success).unwrap();
        write_frame(&mut server, &rsp).unwrap();

        let back = read_frame(&mut client).unwrap();
        assert_eq!(back.message_type(), HsmsMessageType::SelectRsp);
        assert_eq!(back.header10_bytes()[6], 0xAA);
        assert_eq!(back.header10_bytes()[3], 0x00);
    }

    #[test]
    fn loopback_data_message_body() {
        let (mut a, mut b) = loopback_pair();
        let header = [
            0x00, 0x0A, 0x81, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
        ];
        let body = Secs2::ascii("HELLO").unwrap();
        let msg = HsmsMessage::of_with_body(&header, body).unwrap();
        write_frame(&mut a, &msg).unwrap();

        let got = read_frame(&mut b).unwrap();
        assert!(got.is_data_message());
        assert_eq!(got.get_stream(), 1);
        assert_eq!(got.secs2().get_ascii().unwrap(), "HELLO");
    }

    #[test]
    fn channel_wrapper() {
        let (s, c) = loopback_pair();
        let mut server = HsmsTcpChannel::new(s);
        let mut client = HsmsTcpChannel::new(c);
        let req = HsmsMessage::of(&[0xFF, 0xFF, 0x00, 0x00, 0x00, 0x05, 0, 0, 0, 1]).unwrap();
        client.write_message(&req).unwrap();
        let got = server.read_message().unwrap();
        assert_eq!(got.message_type(), HsmsMessageType::LinktestReq);
    }

    #[test]
    fn detect_terminate_on_close() {
        let (mut server, client) = loopback_pair();
        drop(client);
        let err = read_frame(&mut server).unwrap_err();
        assert_eq!(err, Error::DetectTerminate);
    }
}
