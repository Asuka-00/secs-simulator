//! SECS-II data item type codes (wire format high bits).
//!
//! Source: `Secs4Net.Secs2.Secs2Item`.

/// SECS-II item type: code is the wire format high bits (low 2 bits = length-byte count).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Secs2Item {
    Undefined,
    List,
    Binary,
    Boolean,
    Ascii,
    Jis8,
    Unicode,
    Int8,
    Int1,
    Int2,
    Int4,
    Float8,
    Float4,
    Uint8,
    Uint1,
    Uint2,
    Uint4,
}

impl Secs2Item {
    /// Wire format code (without length-byte count bits).
    pub const fn code(self) -> u8 {
        match self {
            Self::Undefined => 0xFF,
            Self::List => 0x00,
            Self::Binary => 0x20,
            Self::Boolean => 0x24,
            Self::Ascii => 0x40,
            Self::Jis8 => 0x44,
            Self::Unicode => 0x48,
            Self::Int8 => 0x60,
            Self::Int1 => 0x64,
            Self::Int2 => 0x68,
            Self::Int4 => 0x70,
            Self::Float8 => 0x80,
            Self::Float4 => 0x90,
            Self::Uint8 => 0xA0,
            Self::Uint1 => 0xA4,
            Self::Uint2 => 0xA8,
            Self::Uint4 => 0xB0,
        }
    }

    /// Element width in bytes for numeric/array body; list/undefined → -1.
    pub const fn size(self) -> i32 {
        match self {
            Self::Undefined | Self::List => -1,
            Self::Binary | Self::Boolean | Self::Ascii | Self::Jis8 | Self::Int1 | Self::Uint1 => 1,
            Self::Unicode | Self::Int2 | Self::Uint2 => 2,
            Self::Int4 | Self::Float4 | Self::Uint4 => 4,
            Self::Int8 | Self::Float8 | Self::Uint8 => 8,
        }
    }

    /// SML-style symbol.
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Undefined => "UNDEFINED",
            Self::List => "L",
            Self::Binary => "B",
            Self::Boolean => "BOOLEAN",
            Self::Ascii => "A",
            Self::Jis8 => "J",
            Self::Unicode => "UNICODE",
            Self::Int8 => "I8",
            Self::Int1 => "I1",
            Self::Int2 => "I2",
            Self::Int4 => "I4",
            Self::Float8 => "F8",
            Self::Float4 => "F4",
            Self::Uint8 => "U8",
            Self::Uint1 => "U1",
            Self::Uint2 => "U2",
            Self::Uint4 => "U4",
        }
    }

    /// Lookup by format byte (masks off low 2 length bits). `Secs2Item.Get(byte)`.
    pub fn from_code(item_code: u8) -> Self {
        let b = item_code & 0xFC;
        match b {
            0x00 => Self::List,
            0x20 => Self::Binary,
            0x24 => Self::Boolean,
            0x40 => Self::Ascii,
            0x44 => Self::Jis8,
            0x48 => Self::Unicode,
            0x60 => Self::Int8,
            0x64 => Self::Int1,
            0x68 => Self::Int2,
            0x70 => Self::Int4,
            0x80 => Self::Float8,
            0x90 => Self::Float4,
            0xA0 => Self::Uint8,
            0xA4 => Self::Uint1,
            0xA8 => Self::Uint2,
            0xB0 => Self::Uint4,
            _ => Self::Undefined,
        }
    }

    /// Lookup by SML symbol (case-insensitive).
    pub fn from_symbol(symbol: &str) -> Self {
        for item in Self::all() {
            if item.symbol().eq_ignore_ascii_case(symbol) {
                return *item;
            }
        }
        Self::Undefined
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Undefined,
            Self::List,
            Self::Binary,
            Self::Boolean,
            Self::Ascii,
            Self::Jis8,
            Self::Unicode,
            Self::Int8,
            Self::Int1,
            Self::Int2,
            Self::Int4,
            Self::Float8,
            Self::Float4,
            Self::Uint8,
            Self::Uint1,
            Self::Uint2,
            Self::Uint4,
        ]
    }
}
