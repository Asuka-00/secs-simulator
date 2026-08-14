//! GEM acknowledge codes (byte → SECS-II Binary).
//!
//! Source: `Secs4Net.Gem` sealed ACK classes (`COMMACK`, `ONLACK`, `HCACK`, …).
//! Shape: idiomatic `enum` + `code` / `secs2` / `from_code` (parity with `Get(byte)`).

use crate::secs2::Secs2;

/// Build `Binary` single-byte SECS-II for an ACK code.
fn bin(code: u8) -> Secs2 {
    Secs2::binary([code]).expect("single-byte binary")
}

macro_rules! gem_ack {
    (
        $(#[$meta:meta])*
        $name:ident {
            $($variant:ident = $code:expr),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)+
            /// Sentinel for unknown / undefined codes (`0xFF` or unmatched).
            Undefined,
        }

        impl $name {
            /// Raw byte code.
            pub const fn code(self) -> u8 {
                match self {
                    $(Self::$variant => $code,)+
                    Self::Undefined => 0xFF,
                }
            }

            /// SECS-II Binary representation (`S2.Binary(code)`).
            pub fn secs2(self) -> Secs2 {
                bin(self.code())
            }

            /// Lookup by code; unknown → `Undefined` (skips sentinel during match).
            pub const fn from_code(b: u8) -> Self {
                match b {
                    $($code => Self::$variant,)+
                    _ => Self::Undefined,
                }
            }

            /// Lookup from SECS-II Binary first byte (`Get(Secs2)`).
            pub fn from_secs2(value: &Secs2) -> crate::secs2::Result<Self> {
                Ok(Self::from_code(value.get_byte_at(&[0])?))
            }
        }
    };
}

gem_ack! {
    /// COMMACK — S1F14 establish communications.
    CommAck {
        Ok = 0x00,
        Denied = 0x01,
    }
}

gem_ack! {
    /// ONLACK — S1F18 online request.
    OnlAck {
        Ok = 0x00,
    }
}

gem_ack! {
    /// OFLACK — S1F16 offline request.
    OflAck {
        Ok = 0x00,
    }
}

gem_ack! {
    /// TIACK — S2F32 date/time set.
    TiAck {
        Ok = 0x00,
        NotDone = 0x01,
    }
}

gem_ack! {
    /// ACKC3 — S3 material status.
    Ackc3 {
        Ok = 0x00,
    }
}

gem_ack! {
    /// ACKC5 — S5 alarm.
    Ackc5 {
        Ok = 0x00,
    }
}

gem_ack! {
    /// ACKC6 — S6 data collection.
    Ackc6 {
        Ok = 0x00,
    }
}

gem_ack! {
    /// ACKC7 — S7 process program.
    Ackc7 {
        Accepted = 0x00,
        PermissionNotGranted = 0x01,
        LengthError = 0x02,
        MatrixOverflow = 0x03,
        PpidNotFound = 0x04,
        UnsupportedMode = 0x05,
        OtherError = 0x06,
    }
}

gem_ack! {
    /// ACKC10 — S10 terminal services.
    Ackc10 {
        AcceptedForDisplay = 0x00,
        MessageWillNotBeDisplayed = 0x01,
        TerminalNotAvailable = 0x02,
    }
}

gem_ack! {
    /// HCACK — host command.
    HcAck {
        Ok = 0x00,
        InvalidCommand = 0x01,
        CannotDoNow = 0x02,
        ParameterError = 0x03,
        InitiatedForAsynchronousCompletion = 0x04,
        RejectedAlreadyInDesiredCondition = 0x05,
        InvalidObject = 0x06,
    }
}

gem_ack! {
    /// GRANT — multi-block inquire grant.
    Grant {
        Ok = 0x00,
        Busy = 0x01,
    }
}

gem_ack! {
    /// GRANT6 — S6 multi-block grant.
    Grant6 {
        Ok = 0x00,
        Busy = 0x01,
        NotInterested = 0x02,
    }
}

gem_ack! {
    /// CMDA — remote command.
    Cmda {
        Ok = 0x00,
        CommandDoesNotExist = 0x01,
        NotNow = 0x02,
    }
}

gem_ack! {
    /// DRACK — define report.
    Drack {
        Ok = 0x00,
        OutOfSpace = 0x01,
        InvalidFormat = 0x02,
        OneOrMoreRptidAlreadyDefined = 0x03,
        OneOrMoreInvalidVid = 0x04,
    }
}

gem_ack! {
    /// ERACK — enable/disable event report.
    Erack {
        Ok = 0x00,
        Denied = 0x01,
    }
}

gem_ack! {
    /// LRACK — link event report.
    Lrack {
        Ok = 0x00,
        OutOfSpace = 0x01,
        InvalidFormat = 0x02,
        OneOrMoreCeidLinksAlreadyDefined = 0x03,
        OneOrMoreCeidInvalid = 0x04,
        OneOrMoreRptidInvalid = 0x05,
    }
}

/// CEED — collection event enable/disable (BOOLEAN, not a byte ACK).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ceed {
    Enable,
    Disable,
}

impl Ceed {
    pub fn secs2(self) -> Secs2 {
        let v = matches!(self, Self::Enable);
        Secs2::bool_values([v]).expect("boolean")
    }

    pub fn from_secs2(value: &Secs2) -> crate::secs2::Result<Self> {
        Ok(if value.get_boolean_at(&[0])? {
            Self::Enable
        } else {
            Self::Disable
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commack_codes_and_roundtrip() {
        assert_eq!(CommAck::Ok.code(), 0x00);
        assert_eq!(CommAck::Denied.code(), 0x01);
        assert_eq!(CommAck::Undefined.code(), 0xFF);
        assert_eq!(CommAck::from_code(0x00), CommAck::Ok);
        assert_eq!(CommAck::from_code(0x01), CommAck::Denied);
        assert_eq!(CommAck::from_code(0x99), CommAck::Undefined);
        assert_eq!(CommAck::from_code(0xFF), CommAck::Undefined);

        let s = CommAck::Denied.secs2();
        assert_eq!(s.get_byte_at(&[0]).unwrap(), 0x01);
        assert_eq!(CommAck::from_secs2(&s).unwrap(), CommAck::Denied);
    }

    #[test]
    fn onlack_oflack_ok_only() {
        assert_eq!(OnlAck::Ok.code(), 0);
        assert_eq!(OflAck::Ok.code(), 0);
        assert_eq!(OnlAck::from_code(1), OnlAck::Undefined);
        assert_eq!(OflAck::from_code(0), OflAck::Ok);
    }

    #[test]
    fn hcack_full_set() {
        assert_eq!(HcAck::InvalidCommand.code(), 1);
        assert_eq!(HcAck::CannotDoNow.code(), 2);
        assert_eq!(HcAck::ParameterError.code(), 3);
        assert_eq!(HcAck::InitiatedForAsynchronousCompletion.code(), 4);
        assert_eq!(HcAck::RejectedAlreadyInDesiredCondition.code(), 5);
        assert_eq!(HcAck::InvalidObject.code(), 6);
        assert_eq!(HcAck::from_code(6), HcAck::InvalidObject);
        assert_eq!(HcAck::from_code(7), HcAck::Undefined);
    }

    #[test]
    fn ackc7_ackc10_grant() {
        assert_eq!(Ackc7::PpidNotFound.code(), 4);
        assert_eq!(Ackc7::from_code(4), Ackc7::PpidNotFound);
        assert_eq!(Ackc10::TerminalNotAvailable.code(), 2);
        assert_eq!(Grant::Busy.code(), 1);
        assert_eq!(Grant6::NotInterested.code(), 2);
        assert_eq!(TiAck::NotDone.code(), 1);
    }

    #[test]
    fn drack_erack_lrack_cmda() {
        assert_eq!(Drack::OneOrMoreInvalidVid.code(), 4);
        assert_eq!(Erack::Denied.code(), 1);
        assert_eq!(Lrack::OneOrMoreRptidInvalid.code(), 5);
        assert_eq!(Cmda::NotNow.code(), 2);
        let s = Erack::Denied.secs2();
        assert_eq!(Erack::from_secs2(&s).unwrap(), Erack::Denied);
    }

    #[test]
    fn ceed_boolean() {
        let e = Ceed::Enable.secs2();
        assert!(e.get_boolean_at(&[0]).unwrap());
        assert_eq!(Ceed::from_secs2(&e).unwrap(), Ceed::Enable);
        let d = Ceed::Disable.secs2();
        assert!(!d.get_boolean_at(&[0]).unwrap());
        assert_eq!(Ceed::from_secs2(&d).unwrap(), Ceed::Disable);
    }

    #[test]
    fn binary_wire_true_ff_not_ack() {
        // ACK uses Binary(code); unrelated to BOOLEAN TRUE=0xFF.
        let s = CommAck::Ok.secs2();
        assert_eq!(s.get_byte_at(&[0]).unwrap(), 0x00);
    }
}
