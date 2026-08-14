//! SECS-I message block (length + 10-byte header + body + checksum).
//!
//! Source: `Secs1MessageBlock` / `AbstractSecs1MessageBlock`.

/// One SECS-I wire block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secs1MessageBlock {
    bytes: Vec<u8>,
    valid: bool,
    length: i32,
    device_id: i32,
    ebit: bool,
    block_number: i32,
    is_first: bool,
}

impl Secs1MessageBlock {
    pub const ZERO: i32 = 0;
    pub const ONE: i32 = 1;

    /// Parse raw block bytes (`Secs1MessageBlock.Of`).
    pub fn of(bs: impl Into<Vec<u8>>) -> Self {
        let bs = bs.into();
        let mut length = -1_i32;
        let mut valid = false;

        // Frame: length(1) + body(length) + checksum(2); total 13..=257.
        if (13..=257).contains(&bs.len()) {
            length = i32::from(bs[0]);
            if (10..=254).contains(&length) && (length as usize + 3) == bs.len() {
                let mut i = length as usize;
                let mut v = ((i32::from(bs[i + 1]) << 8) & 0x0000_FF00)
                    | (i32::from(bs[i + 2]) & 0x0000_00FF);
                while i > 0 {
                    v -= i32::from(bs[i]) & 0xFF;
                    i -= 1;
                }
                valid = v == 0;
            }
        }

        if valid {
            let device_id = ((i32::from(bs[1]) << 8) & 0x0000_7F00) | (i32::from(bs[2]) & 0xFF);
            let ebit = (i32::from(bs[5]) & 0x80) == 0x80;
            let block_number =
                ((i32::from(bs[5]) << 8) & 0x0000_7F00) | (i32::from(bs[6]) & 0xFF);
            let is_first = block_number == Self::ZERO || block_number == Self::ONE;
            Self {
                bytes: bs,
                valid: true,
                length,
                device_id,
                ebit,
                block_number,
                is_first,
            }
        } else {
            Self {
                bytes: bs,
                valid: false,
                length: -1,
                device_id: -1,
                ebit: false,
                block_number: -1,
                is_first: false,
            }
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn device_id(&self) -> i32 {
        self.device_id
    }

    pub fn ebit(&self) -> bool {
        self.ebit
    }

    pub fn block_number(&self) -> i32 {
        self.block_number
    }

    pub fn is_first_block(&self) -> bool {
        self.is_first
    }

    pub fn length(&self) -> i32 {
        self.length
    }

    pub fn get_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Checksum result (== is_valid).
    pub fn check_sum(&self) -> bool {
        self.valid
    }

    pub fn equals_system_bytes(&self, other: &Self) -> bool {
        if self.valid && other.valid && self.bytes.len() >= 11 && other.bytes.len() >= 11 {
            self.bytes[7] == other.bytes[7]
                && self.bytes[8] == other.bytes[8]
                && self.bytes[9] == other.bytes[9]
                && self.bytes[10] == other.bytes[10]
        } else {
            false
        }
    }

    pub fn is_next_block(&self, next: &Self) -> bool {
        self.valid && next.valid && next.block_number == self.block_number + 1
    }

    /// Body slice inside the block (after 10-byte header, before 2-byte checksum).
    pub fn body_bytes(&self) -> Option<&[u8]> {
        if !self.valid {
            return None;
        }
        let end = self.bytes.len() - 2;
        if end >= 11 {
            Some(&self.bytes[11..end])
        } else {
            None
        }
    }
}
