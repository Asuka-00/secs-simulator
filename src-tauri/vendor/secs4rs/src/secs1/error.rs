//! SECS-I error kinds.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    HeaderByteLength,
    EmptyBlockList,
    TooBigMessageBody,
    InvalidBlocks,
    /// Length byte outside 10..=254.
    IllegalLengthByte(i32),
    /// Checksum failed after full frame read.
    ChecksumMismatch,
    /// Peer closed during read.
    DetectTerminate,
    /// T2 timeout (EOT/ACK/length).
    TimeoutT2,
    /// T1 timeout (inter-character within block).
    TimeoutT1,
    /// T4 timeout (inter-block: next ENQ).
    TimeoutT4,
    /// Expected ACK, got other byte.
    NotReceiveAck(u8),
    /// Expected EOT, got other byte.
    NotReceiveEot(u8),
    /// After intermediate block, next control byte was not ENQ.
    NotReceiveNextBlockEnq(u8),
    /// ENQ/EOT/ACK retries exhausted.
    RetryOver,
    /// Slave yielded: peer ENQ arrived while waiting for EOT.
    PeerEnq,
    /// T3 timeout waiting for reply (W-bit).
    TimeoutT3,
    /// Circuit / channel shut down.
    ChannelShutdown,
    /// I/O error.
    Io(String),
    /// Protocol violation.
    Protocol(&'static str),
    Secs2(crate::secs2::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeaderByteLength => write!(f, "SECS-I header must be 10 bytes"),
            Self::EmptyBlockList => write!(f, "empty block list"),
            Self::TooBigMessageBody => write!(f, "SECS-I message body too big"),
            Self::InvalidBlocks => write!(f, "invalid SECS-I block sequence"),
            Self::IllegalLengthByte(n) => write!(f, "SECS-I illegal length byte: {n}"),
            Self::ChecksumMismatch => write!(f, "SECS-I checksum mismatch"),
            Self::DetectTerminate => write!(f, "SECS-I detect terminate"),
            Self::TimeoutT2 => write!(f, "SECS-I T2 timeout"),
            Self::TimeoutT1 => write!(f, "SECS-I T1 timeout"),
            Self::TimeoutT4 => write!(f, "SECS-I T4 timeout"),
            Self::NotReceiveAck(b) => write!(f, "SECS-I not receive ACK, got {b:#04x}"),
            Self::NotReceiveEot(b) => write!(f, "SECS-I not receive EOT, got {b:#04x}"),
            Self::NotReceiveNextBlockEnq(b) => {
                write!(f, "SECS-I not receive next-block ENQ, got {b:#04x}")
            }
            Self::RetryOver => write!(f, "SECS-I retry over"),
            Self::PeerEnq => write!(f, "SECS-I peer ENQ (slave yield)"),
            Self::TimeoutT3 => write!(f, "SECS-I T3 timeout"),
            Self::ChannelShutdown => write!(f, "SECS-I channel shutdown"),
            Self::Io(m) => write!(f, "SECS-I I/O: {m}"),
            Self::Protocol(m) => write!(f, "SECS-I: {m}"),
            Self::Secs2(e) => write!(f, "SECS-II: {e}"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Self::DetectTerminate
        } else if e.kind() == std::io::ErrorKind::TimedOut
            || e.kind() == std::io::ErrorKind::WouldBlock
        {
            Self::Io(format!("timeout: {e}"))
        } else {
            Self::Io(e.to_string())
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
