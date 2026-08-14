//! SECS-II data items: idiomatic `enum Secs2` with wire parity against Secs4Net.
//!
//! Hard constraints: format|len|data layout, TRUE=`0xFF`, Empty size=`-1`,
//! big-endian multi-byte numbers, `get_bytes_list` chunk boundaries.

mod error;
mod item;

pub use error::{Error, Result};
pub use item::Secs2Item;

const MAX_BODY: usize = 0x00FF_FFFF;
const BYTE_TRUE: u8 = 0xFF;
const BYTE_FALSE: u8 = 0x00;

/// SECS-II value tree.
///
/// Maps the Secs4Net multi-class hierarchy onto a single closed enum.
#[derive(Debug, Clone, PartialEq)]
pub enum Secs2 {
    /// Raw/empty payload (`Secs2RawBytes` / `Empty()`). `size() == -1`.
    Empty,
    List(Vec<Secs2>),
    Ascii(String),
    Binary(Vec<u8>),
    Boolean(Vec<bool>),
    Int1(Vec<i8>),
    Int2(Vec<i16>),
    Int4(Vec<i32>),
    Int8(Vec<i64>),
    Uint1(Vec<u8>),
    Uint2(Vec<u16>),
    Uint4(Vec<u32>),
    Uint8(Vec<u64>),
    Float4(Vec<f32>),
    Float8(Vec<f64>),
    /// Parsed JIS8 body (opaque bytes for now).
    Jis8(Vec<u8>),
    /// Parsed UNICODE body (opaque UTF-16BE bytes).
    Unicode(Vec<u8>),
}

// ─── constructors (Secs2 static factories) ───────────────────────────────────

impl Secs2 {
    pub fn empty() -> Self {
        Self::Empty
    }

    pub fn list(values: impl IntoIterator<Item = Secs2>) -> Result<Self> {
        let v: Vec<Secs2> = values.into_iter().collect();
        if v.len() > MAX_BODY {
            return Err(Error::LengthByteOutOfRange);
        }
        Ok(Self::List(v))
    }

    pub fn list_empty() -> Self {
        Self::List(Vec::new())
    }

    pub fn ascii(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if s.len() > MAX_BODY {
            return Err(Error::LengthByteOutOfRange);
        }
        // SECS-II ASCII: 7-bit; encode as Latin-1/ASCII bytes of the string chars.
        Ok(Self::Ascii(s))
    }

    pub fn binary(bs: impl Into<Vec<u8>>) -> Result<Self> {
        let bs = bs.into();
        if bs.len() > MAX_BODY {
            return Err(Error::LengthByteOutOfRange);
        }
        Ok(Self::Binary(bs))
    }

    pub fn bool_values(vals: impl IntoIterator<Item = bool>) -> Result<Self> {
        let v: Vec<bool> = vals.into_iter().collect();
        if v.len() > MAX_BODY {
            return Err(Error::LengthByteOutOfRange);
        }
        Ok(Self::Boolean(v))
    }

    pub fn bool1(v: bool) -> Self {
        Self::Boolean(vec![v])
    }

    pub fn int1(vals: impl IntoIterator<Item = i8>) -> Result<Self> {
        let v: Vec<i8> = vals.into_iter().collect();
        check_num_len(v.len(), 1)?;
        Ok(Self::Int1(v))
    }

    pub fn int1_from_i32(vals: impl IntoIterator<Item = i32>) -> Result<Self> {
        Self::int1(vals.into_iter().map(|x| x as i8))
    }

    pub fn int2(vals: impl IntoIterator<Item = i16>) -> Result<Self> {
        let v: Vec<i16> = vals.into_iter().collect();
        check_num_len(v.len(), 2)?;
        Ok(Self::Int2(v))
    }

    pub fn int4(vals: impl IntoIterator<Item = i32>) -> Result<Self> {
        let v: Vec<i32> = vals.into_iter().collect();
        check_num_len(v.len(), 4)?;
        Ok(Self::Int4(v))
    }

    pub fn int8(vals: impl IntoIterator<Item = i64>) -> Result<Self> {
        let v: Vec<i64> = vals.into_iter().collect();
        check_num_len(v.len(), 8)?;
        Ok(Self::Int8(v))
    }

    pub fn uint1(vals: impl IntoIterator<Item = u8>) -> Result<Self> {
        let v: Vec<u8> = vals.into_iter().collect();
        check_num_len(v.len(), 1)?;
        Ok(Self::Uint1(v))
    }

    pub fn uint2(vals: impl IntoIterator<Item = u16>) -> Result<Self> {
        let v: Vec<u16> = vals.into_iter().collect();
        check_num_len(v.len(), 2)?;
        Ok(Self::Uint2(v))
    }

    pub fn uint4(vals: impl IntoIterator<Item = u32>) -> Result<Self> {
        let v: Vec<u32> = vals.into_iter().collect();
        check_num_len(v.len(), 4)?;
        Ok(Self::Uint4(v))
    }

    pub fn uint8(vals: impl IntoIterator<Item = u64>) -> Result<Self> {
        let v: Vec<u64> = vals.into_iter().collect();
        check_num_len(v.len(), 8)?;
        Ok(Self::Uint8(v))
    }

    pub fn float4(vals: impl IntoIterator<Item = f32>) -> Result<Self> {
        let v: Vec<f32> = vals.into_iter().collect();
        check_num_len(v.len(), 4)?;
        Ok(Self::Float4(v))
    }

    pub fn float8(vals: impl IntoIterator<Item = f64>) -> Result<Self> {
        let v: Vec<f64> = vals.into_iter().collect();
        check_num_len(v.len(), 8)?;
        Ok(Self::Float8(v))
    }
}

fn check_num_len(count: usize, elem: usize) -> Result<()> {
    if elem == 0 {
        return Ok(());
    }
    // Secs2BigInteger: count >= (0x01000000 / size)
    if count >= (0x0100_0000 / elem) {
        return Err(Error::LengthByteOutOfRange);
    }
    Ok(())
}

// ─── accessors ───────────────────────────────────────────────────────────────

impl Secs2 {
    pub fn secs2_item(&self) -> Secs2Item {
        match self {
            Self::Empty => Secs2Item::Undefined,
            Self::List(_) => Secs2Item::List,
            Self::Ascii(_) => Secs2Item::Ascii,
            Self::Binary(_) => Secs2Item::Binary,
            Self::Boolean(_) => Secs2Item::Boolean,
            Self::Int1(_) => Secs2Item::Int1,
            Self::Int2(_) => Secs2Item::Int2,
            Self::Int4(_) => Secs2Item::Int4,
            Self::Int8(_) => Secs2Item::Int8,
            Self::Uint1(_) => Secs2Item::Uint1,
            Self::Uint2(_) => Secs2Item::Uint2,
            Self::Uint4(_) => Secs2Item::Uint4,
            Self::Uint8(_) => Secs2Item::Uint8,
            Self::Float4(_) => Secs2Item::Float4,
            Self::Float8(_) => Secs2Item::Float8,
            Self::Jis8(_) => Secs2Item::Jis8,
            Self::Unicode(_) => Secs2Item::Unicode,
        }
    }

    /// Element count. **Empty raw → -1** (Java/C# parity).
    pub fn size(&self) -> i32 {
        match self {
            Self::Empty => -1,
            Self::List(v) => v.len() as i32,
            Self::Ascii(s) => s.len() as i32,
            Self::Binary(v) => v.len() as i32,
            Self::Boolean(v) => v.len() as i32,
            Self::Int1(v) => v.len() as i32,
            Self::Int2(v) => v.len() as i32,
            Self::Int4(v) => v.len() as i32,
            Self::Int8(v) => v.len() as i32,
            Self::Uint1(v) => v.len() as i32,
            Self::Uint2(v) => v.len() as i32,
            Self::Uint4(v) => v.len() as i32,
            Self::Uint8(v) => v.len() as i32,
            Self::Float4(v) => v.len() as i32,
            Self::Float8(v) => v.len() as i32,
            Self::Jis8(v) | Self::Unicode(v) => v.len() as i32,
        }
    }

    pub fn is_empty_item(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Navigate by full index path to a nested item.
    pub fn get_item(&self, indices: &[usize]) -> Result<&Secs2> {
        if indices.is_empty() {
            return Ok(self);
        }
        match self {
            Self::List(items) => {
                let idx = indices[0];
                let child = items.get(idx).ok_or(Error::IndexOutOfBounds)?;
                child.get_item(&indices[1..])
            }
            _ => Err(Error::IllegalDataFormat("Not Secs2List")),
        }
    }

    pub fn get_ascii(&self) -> Result<&str> {
        match self {
            Self::Ascii(s) => Ok(s.as_str()),
            _ => Err(Error::IllegalDataFormat("Not Secs2Ascii")),
        }
    }

    pub fn get_ascii_at(&self, indices: &[usize]) -> Result<String> {
        Ok(self.get_item(indices)?.get_ascii()?.to_string())
    }

    pub fn get_byte_at(&self, indices: &[usize]) -> Result<u8> {
        let (path, last) = split_last(indices)?;
        self.get_item(path)?.get_byte(*last)
    }

    fn get_byte(&self, index: usize) -> Result<u8> {
        match self {
            Self::Binary(v) => v.get(index).copied().ok_or(Error::IndexOutOfBounds),
            Self::Uint1(v) => v.get(index).copied().ok_or(Error::IndexOutOfBounds),
            Self::Int1(v) => v
                .get(index)
                .map(|x| *x as u8)
                .ok_or(Error::IndexOutOfBounds),
            _ => Err(Error::IllegalDataFormat("Not Secs2Byte")),
        }
    }

    pub fn get_boolean_at(&self, indices: &[usize]) -> Result<bool> {
        let (path, last) = split_last(indices)?;
        match self.get_item(path)? {
            Self::Boolean(v) => v.get(*last).copied().ok_or(Error::IndexOutOfBounds),
            _ => Err(Error::IllegalDataFormat("Not Secs2Boolean")),
        }
    }

    pub fn get_int_at(&self, indices: &[usize]) -> Result<i32> {
        Ok(self.get_long_at(indices)? as i32)
    }

    pub fn get_long_at(&self, indices: &[usize]) -> Result<i64> {
        let (path, last) = split_last(indices)?;
        self.get_item(path)?.get_long(*last)
    }

    fn get_long(&self, index: usize) -> Result<i64> {
        match self {
            Self::Int1(v) => v
                .get(index)
                .map(|x| i64::from(*x))
                .ok_or(Error::IndexOutOfBounds),
            Self::Int2(v) => v
                .get(index)
                .map(|x| i64::from(*x))
                .ok_or(Error::IndexOutOfBounds),
            Self::Int4(v) => v
                .get(index)
                .map(|x| i64::from(*x))
                .ok_or(Error::IndexOutOfBounds),
            Self::Int8(v) => v.get(index).copied().ok_or(Error::IndexOutOfBounds),
            Self::Uint1(v) => v
                .get(index)
                .map(|x| i64::from(*x))
                .ok_or(Error::IndexOutOfBounds),
            Self::Uint2(v) => v
                .get(index)
                .map(|x| i64::from(*x))
                .ok_or(Error::IndexOutOfBounds),
            Self::Uint4(v) => v
                .get(index)
                .map(|x| i64::from(*x))
                .ok_or(Error::IndexOutOfBounds),
            Self::Uint8(v) => v
                .get(index)
                .map(|x| *x as i64)
                .ok_or(Error::IndexOutOfBounds),
            Self::Binary(v) => v
                .get(index)
                .map(|x| i64::from(*x as i8))
                .ok_or(Error::IndexOutOfBounds),
            _ => Err(Error::IllegalDataFormat("Not Secs2Number")),
        }
    }

    pub fn get_float_at(&self, indices: &[usize]) -> Result<f32> {
        let (path, last) = split_last(indices)?;
        match self.get_item(path)? {
            Self::Float4(v) => v.get(*last).copied().ok_or(Error::IndexOutOfBounds),
            Self::Float8(v) => v
                .get(*last)
                .map(|x| *x as f32)
                .ok_or(Error::IndexOutOfBounds),
            other => Ok(other.get_long(*last)? as f32),
        }
    }

    pub fn get_double_at(&self, indices: &[usize]) -> Result<f64> {
        let (path, last) = split_last(indices)?;
        match self.get_item(path)? {
            Self::Float8(v) => v.get(*last).copied().ok_or(Error::IndexOutOfBounds),
            Self::Float4(v) => v
                .get(*last)
                .map(|x| f64::from(*x))
                .ok_or(Error::IndexOutOfBounds),
            other => Ok(other.get_long(*last)? as f64),
        }
    }
}

fn split_last(indices: &[usize]) -> Result<(&[usize], &usize)> {
    // slice::split_last → (last, rest); we need (path, last_index).
    let (last, path) = indices.split_last().ok_or(Error::IndexOutOfBounds)?;
    Ok((path, last))
}

// ─── encode ──────────────────────────────────────────────────────────────────

impl Secs2 {
    /// Encode into chunks of at most `max_bytes_size` (Secs4Net `GetBytesList`).
    pub fn get_bytes_list(&self, max_bytes_size: usize) -> Vec<Vec<u8>> {
        if matches!(self, Self::Empty) {
            return vec![Vec::new()];
        }
        let mut b = BytesListBuilder::new(max_bytes_size);
        self.put_bytes_pack(&mut b);
        b.finish()
    }

    /// Flatten all chunks into one buffer (test helper; not a Secs4Net API).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.get_bytes_list(usize::MAX / 4)
            .into_iter()
            .flatten()
            .collect()
    }

    fn put_bytes_pack(&self, b: &mut BytesListBuilder) {
        match self {
            Self::Empty => {}
            Self::List(items) => {
                put_header(b, Secs2Item::List, items.len());
                for child in items {
                    // Children encode with 1024-byte internal chunks then stream in
                    // (Secs2List.PutBytesPack parity).
                    for chunk in child.get_bytes_list(1024) {
                        b.put_slice(&chunk);
                    }
                }
            }
            Self::Ascii(s) => {
                let body = s.as_bytes();
                put_header(b, Secs2Item::Ascii, body.len());
                b.put_slice(body);
            }
            Self::Binary(v) => {
                put_header(b, Secs2Item::Binary, v.len());
                b.put_slice(v);
            }
            Self::Boolean(v) => {
                put_header(b, Secs2Item::Boolean, v.len());
                for &x in v {
                    b.put_u8(if x { BYTE_TRUE } else { BYTE_FALSE });
                }
            }
            Self::Int1(v) => {
                put_header(b, Secs2Item::Int1, v.len());
                for &x in v {
                    b.put_u8(x as u8);
                }
            }
            Self::Int2(v) => {
                put_header(b, Secs2Item::Int2, v.len() * 2);
                for &x in v {
                    b.put_slice(&x.to_be_bytes());
                }
            }
            Self::Int4(v) => {
                put_header(b, Secs2Item::Int4, v.len() * 4);
                for &x in v {
                    b.put_slice(&x.to_be_bytes());
                }
            }
            Self::Int8(v) => {
                put_header(b, Secs2Item::Int8, v.len() * 8);
                for &x in v {
                    b.put_slice(&x.to_be_bytes());
                }
            }
            Self::Uint1(v) => {
                put_header(b, Secs2Item::Uint1, v.len());
                b.put_slice(v);
            }
            Self::Uint2(v) => {
                put_header(b, Secs2Item::Uint2, v.len() * 2);
                for &x in v {
                    b.put_slice(&x.to_be_bytes());
                }
            }
            Self::Uint4(v) => {
                put_header(b, Secs2Item::Uint4, v.len() * 4);
                for &x in v {
                    b.put_slice(&x.to_be_bytes());
                }
            }
            Self::Uint8(v) => {
                put_header(b, Secs2Item::Uint8, v.len() * 8);
                for &x in v {
                    b.put_slice(&x.to_be_bytes());
                }
            }
            Self::Float4(v) => {
                put_header(b, Secs2Item::Float4, v.len() * 4);
                for &x in v {
                    b.put_slice(&x.to_bits().to_be_bytes());
                }
            }
            Self::Float8(v) => {
                put_header(b, Secs2Item::Float8, v.len() * 8);
                for &x in v {
                    b.put_slice(&x.to_bits().to_be_bytes());
                }
            }
            Self::Jis8(v) | Self::Unicode(v) => {
                put_header(b, self.secs2_item(), v.len());
                b.put_slice(v);
            }
        }
    }
}

fn put_header(b: &mut BytesListBuilder, item: Secs2Item, length: usize) {
    let code = item.code();
    if length > 0xFFFF {
        b.put_slice(&[
            code | 0x3,
            ((length >> 16) & 0xFF) as u8,
            ((length >> 8) & 0xFF) as u8,
            (length & 0xFF) as u8,
        ]);
    } else if length > 0xFF {
        b.put_slice(&[
            code | 0x2,
            ((length >> 8) & 0xFF) as u8,
            (length & 0xFF) as u8,
        ]);
    } else {
        b.put_slice(&[code | 0x1, (length & 0xFF) as u8]);
    }
}

struct BytesListBuilder {
    max: usize,
    chunks: Vec<Vec<u8>>,
    cur: Vec<u8>,
}

impl BytesListBuilder {
    fn new(max_bytes_size: usize) -> Self {
        let max = max_bytes_size.max(1);
        Self {
            max,
            chunks: Vec::new(),
            cur: Vec::with_capacity(max.min(256)),
        }
    }

    fn put_u8(&mut self, byte: u8) {
        self.put_slice(&[byte]);
    }

    fn put_slice(&mut self, bs: &[u8]) {
        let mut i = 0;
        let m = bs.len();
        while i < m {
            let room = self.max - self.cur.len();
            if m - i > room {
                self.cur.extend_from_slice(&bs[i..i + room]);
                i += room;
                self.flush_full();
            } else {
                self.cur.extend_from_slice(&bs[i..m]);
                i = m;
            }
        }
    }

    fn flush_full(&mut self) {
        let full = std::mem::replace(&mut self.cur, Vec::with_capacity(self.max.min(256)));
        self.chunks.push(full);
    }

    fn finish(mut self) -> Vec<Vec<u8>> {
        // Always include trailing partial chunk (may be empty), matching C#.
        self.chunks.push(self.cur);
        self.chunks
    }
}

// ─── parse ───────────────────────────────────────────────────────────────────

impl Secs2 {
    /// Parse chunk list into a SECS-II tree (`Secs2BytesParsers.Parse`).
    pub fn parse(chunks: &[Vec<u8>]) -> Result<Self> {
        let mut pack = BytesPack::new(chunks);
        if !pack.has_remaining() {
            return Ok(Self::Empty);
        }
        let ss = stp_parse(&mut pack)?;
        if pack.has_remaining() {
            return Err(Error::BytesParse("not reach end bytes"));
        }
        Ok(ss)
    }

    pub fn parse_bytes(bytes: &[u8]) -> Result<Self> {
        Self::parse(&[bytes.to_vec()])
    }
}

fn stp_parse(pack: &mut BytesPack<'_>) -> Result<Secs2> {
    let b = pack.get()?;
    let s2i = Secs2Item::from_code(b);
    let length_bits = b & 0x03;
    let size = match length_bits {
        3 => {
            let mut n = (u32::from(pack.get()?) << 16) & 0x00FF_0000;
            n |= (u32::from(pack.get()?) << 8) & 0x0000_FF00;
            n |= u32::from(pack.get()?) & 0x0000_00FF;
            n as usize
        }
        2 => {
            let mut n = (u32::from(pack.get()?) << 8) & 0x0000_FF00;
            n |= u32::from(pack.get()?) & 0x0000_00FF;
            n as usize
        }
        1 => usize::from(pack.get()?),
        _ => 0, // length_bits == 0 → size 0 (unusual but mirror mask path)
    };

    if s2i == Secs2Item::List {
        let mut ll = Vec::with_capacity(size);
        for _ in 0..size {
            ll.push(stp_parse(pack)?);
        }
        return Ok(Secs2::List(ll));
    }

    let bs = pack.get_n(size)?;
    match s2i {
        Secs2Item::Ascii => Ok(Secs2::Ascii(String::from_utf8_lossy(&bs).into_owned())),
        Secs2Item::Binary => Ok(Secs2::Binary(bs)),
        Secs2Item::Boolean => Ok(Secs2::Boolean(bs.iter().map(|&x| x != BYTE_FALSE).collect())),
        Secs2Item::Int1 => Ok(Secs2::Int1(bs.iter().map(|&x| x as i8).collect())),
        Secs2Item::Int2 => Ok(Secs2::Int2(read_i16_be(&bs)?)),
        Secs2Item::Int4 => Ok(Secs2::Int4(read_i32_be(&bs)?)),
        Secs2Item::Int8 => Ok(Secs2::Int8(read_i64_be(&bs)?)),
        Secs2Item::Uint1 => Ok(Secs2::Uint1(bs)),
        Secs2Item::Uint2 => Ok(Secs2::Uint2(read_u16_be(&bs)?)),
        Secs2Item::Uint4 => Ok(Secs2::Uint4(read_u32_be(&bs)?)),
        Secs2Item::Uint8 => Ok(Secs2::Uint8(read_u64_be(&bs)?)),
        Secs2Item::Float4 => Ok(Secs2::Float4(read_f32_be(&bs)?)),
        Secs2Item::Float8 => Ok(Secs2::Float8(read_f64_be(&bs)?)),
        Secs2Item::Jis8 => Ok(Secs2::Jis8(bs)),
        Secs2Item::Unicode => Ok(Secs2::Unicode(bs)),
        Secs2Item::List => unreachable!(),
        Secs2Item::Undefined => Err(Error::UnsupportedDataFormat),
    }
}

fn read_i16_be(bs: &[u8]) -> Result<Vec<i16>> {
    if bs.len() % 2 != 0 {
        return Err(Error::BytesParse("int2 length"));
    }
    Ok(bs
        .chunks_exact(2)
        .map(|c| i16::from_be_bytes([c[0], c[1]]))
        .collect())
}

fn read_i32_be(bs: &[u8]) -> Result<Vec<i32>> {
    if bs.len() % 4 != 0 {
        return Err(Error::BytesParse("int4 length"));
    }
    Ok(bs
        .chunks_exact(4)
        .map(|c| i32::from_be_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

fn read_i64_be(bs: &[u8]) -> Result<Vec<i64>> {
    if bs.len() % 8 != 0 {
        return Err(Error::BytesParse("int8 length"));
    }
    Ok(bs
        .chunks_exact(8)
        .map(|c| {
            i64::from_be_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
        })
        .collect())
}

fn read_u16_be(bs: &[u8]) -> Result<Vec<u16>> {
    if bs.len() % 2 != 0 {
        return Err(Error::BytesParse("uint2 length"));
    }
    Ok(bs
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect())
}

fn read_u32_be(bs: &[u8]) -> Result<Vec<u32>> {
    if bs.len() % 4 != 0 {
        return Err(Error::BytesParse("uint4 length"));
    }
    Ok(bs
        .chunks_exact(4)
        .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

fn read_u64_be(bs: &[u8]) -> Result<Vec<u64>> {
    if bs.len() % 8 != 0 {
        return Err(Error::BytesParse("uint8 length"));
    }
    Ok(bs
        .chunks_exact(8)
        .map(|c| {
            u64::from_be_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
        })
        .collect())
}

fn read_f32_be(bs: &[u8]) -> Result<Vec<f32>> {
    if bs.len() % 4 != 0 {
        return Err(Error::BytesParse("float4 length"));
    }
    Ok(bs
        .chunks_exact(4)
        .map(|c| f32::from_bits(u32::from_be_bytes([c[0], c[1], c[2], c[3]])))
        .collect())
}

fn read_f64_be(bs: &[u8]) -> Result<Vec<f64>> {
    if bs.len() % 8 != 0 {
        return Err(Error::BytesParse("float8 length"));
    }
    Ok(bs
        .chunks_exact(8)
        .map(|c| {
            f64::from_bits(u64::from_be_bytes([
                c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7],
            ]))
        })
        .collect())
}

struct BytesPack<'a> {
    packs: Vec<&'a [u8]>,
    i_pack: usize,
    i_byte: usize,
}

impl<'a> BytesPack<'a> {
    fn new(chunks: &'a [Vec<u8>]) -> Self {
        let packs: Vec<&[u8]> = chunks
            .iter()
            .map(|c| c.as_slice())
            .filter(|c| !c.is_empty())
            .collect();
        Self {
            packs,
            i_pack: 0,
            i_byte: 0,
        }
    }

    fn has_remaining(&self) -> bool {
        if self.packs.is_empty() {
            return false;
        }
        if self.i_pack < self.packs.len() - 1 {
            return true;
        }
        if self.i_pack == self.packs.len() - 1 {
            return self.i_byte < self.packs[self.i_pack].len();
        }
        false
    }

    fn get(&mut self) -> Result<u8> {
        while self.i_pack < self.packs.len() {
            if self.i_byte >= self.packs[self.i_pack].len() {
                self.i_pack += 1;
                self.i_byte = 0;
                continue;
            }
            let b = self.packs[self.i_pack][self.i_byte];
            self.i_byte += 1;
            return Ok(b);
        }
        Err(Error::BytesParse("reach end bytes"))
    }

    fn get_n(&mut self, size: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(size);
        for _ in 0..size {
            out.push(self.get()?);
        }
        Ok(out)
    }
}

// ─── tests (Batch1 + wire batch2 subset) ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secs2_ascii() {
        let a = Secs2::ascii("HELLO").unwrap();
        assert_eq!(a.get_ascii().unwrap(), "HELLO");
        assert_eq!(a.size(), 5);
    }

    #[test]
    fn secs2_int_element_index() {
        let i4 = Secs2::int4([10, 20, 30]).unwrap();
        assert_eq!(i4.get_int_at(&[0]).unwrap(), 10);
        assert_eq!(i4.get_int_at(&[2]).unwrap(), 30);
        assert_eq!(i4.size(), 3);
    }

    #[test]
    fn secs2_list_navigation() {
        let msg = Secs2::list([
            Secs2::ascii("ABC").unwrap(),
            Secs2::int4([1, 2, 3]).unwrap(),
            Secs2::binary(vec![0x10, 0xFF]).unwrap(),
        ])
        .unwrap();
        assert_eq!(msg.size(), 3);
        assert_eq!(msg.get_ascii_at(&[0]).unwrap(), "ABC");
        assert_eq!(msg.get_int_at(&[1, 1]).unwrap(), 2);
        assert_eq!(msg.get_byte_at(&[2, 1]).unwrap(), 0xFF);
    }

    #[test]
    fn secs2_roundtrip() {
        let a = Secs2::list([
            Secs2::ascii("ABC").unwrap(),
            Secs2::int4([1, 2, 3]).unwrap(),
            Secs2::binary(vec![0x10, 0xFF]).unwrap(),
        ])
        .unwrap();
        let bss = a.get_bytes_list(244);
        let b = Secs2::parse(&bss).unwrap();
        assert_eq!(b.size(), 3);
        assert_eq!(b.get_ascii_at(&[0]).unwrap(), "ABC");
        assert_eq!(b.get_int_at(&[1, 1]).unwrap(), 2);
        assert_eq!(b.get_byte_at(&[2, 1]).unwrap(), 0xFF);
    }

    #[test]
    fn secs2_byte_unsigned_wire() {
        assert_eq!(
            Secs2::binary(vec![0xFF])
                .unwrap()
                .get_byte_at(&[0])
                .unwrap(),
            0xFF
        );
        assert_eq!(
            Secs2::binary(vec![0x80])
                .unwrap()
                .get_byte_at(&[0])
                .unwrap(),
            0x80
        );
    }

    #[test]
    fn secs2_empty() {
        assert_eq!(Secs2::empty().size(), -1);
        assert_eq!(Secs2::list_empty().size(), 0);
    }

    #[test]
    fn wire_ascii_ab() {
        let w = Secs2::ascii("AB").unwrap().to_bytes();
        assert_eq!(w, vec![0x41, 0x02, 0x41, 0x42]);
    }

    #[test]
    fn wire_int1_5() {
        let w = Secs2::int1_from_i32([5]).unwrap().to_bytes();
        assert_eq!(w, vec![0x65, 0x01, 0x05]);
    }

    #[test]
    fn wire_bool_true() {
        let w = Secs2::bool1(true).to_bytes();
        assert_eq!(w, vec![0x25, 0x01, 0xFF]);
    }

    #[test]
    fn wire_empty_list() {
        let w = Secs2::list_empty().to_bytes();
        assert_eq!(w, vec![0x01, 0x00]);
    }

    #[test]
    fn bool_getvalue() {
        let b = Secs2::bool_values([true, false, true]).unwrap();
        assert_eq!(b.size(), 3);
        assert!(b.get_boolean_at(&[0]).unwrap());
        assert!(!b.get_boolean_at(&[1]).unwrap());
        assert!(b.get_boolean_at(&[2]).unwrap());
    }

    #[test]
    fn float_roundtrip() {
        let f = Secs2::float4([1.5, -2.25]).unwrap();
        assert_eq!(f.get_float_at(&[0]).unwrap(), 1.5);
        assert_eq!(f.get_float_at(&[1]).unwrap(), -2.25);
        let p = Secs2::parse(&f.get_bytes_list(244)).unwrap();
        assert_eq!(p.get_float_at(&[0]).unwrap(), 1.5);
        assert_eq!(p.get_float_at(&[1]).unwrap(), -2.25);
    }

    #[test]
    fn uint_getlong() {
        let u = Secs2::uint4([1u32, 4_000_000_000u32]).unwrap();
        assert_eq!(u.get_long_at(&[0]).unwrap(), 1);
        assert_eq!(u.get_long_at(&[1]).unwrap(), 4_000_000_000);
    }

    #[test]
    fn int8_long() {
        let i8 = Secs2::int8([9_000_000_000i64]).unwrap();
        assert_eq!(i8.get_long_at(&[0]).unwrap(), 9_000_000_000);
    }

    #[test]
    fn secs2item_codes() {
        assert_eq!(
            Secs2::ascii("x").unwrap().secs2_item(),
            Secs2Item::Ascii
        );
        assert_eq!(Secs2::int4([1]).unwrap().secs2_item(), Secs2Item::Int4);
        assert_eq!(Secs2::bool1(true).secs2_item(), Secs2Item::Boolean);
        assert_eq!(Secs2::list_empty().secs2_item(), Secs2Item::List);
        assert_eq!(Secs2Item::Ascii.code(), 0x40);
        assert_eq!(Secs2Item::Int4.code(), 0x70);
    }

    #[test]
    fn nested_list_roundtrip() {
        let a = Secs2::list([
            Secs2::list([
                Secs2::ascii("inner").unwrap(),
                Secs2::uint2([255u16]).unwrap(),
            ])
            .unwrap(),
            Secs2::bool1(true),
        ])
        .unwrap();
        let b = Secs2::parse(&a.get_bytes_list(244)).unwrap();
        assert_eq!(b.size(), 2);
        assert_eq!(b.get_ascii_at(&[0, 0]).unwrap(), "inner");
        assert_eq!(b.get_int_at(&[0, 1, 0]).unwrap(), 255);
        assert!(b.get_boolean_at(&[1, 0]).unwrap());
    }

    #[test]
    fn multi_chunk_encode_decode() {
        // Force multi-chunk with tiny max; parse must reassemble.
        let ascii = "X".repeat(100);
        let a = Secs2::ascii(&ascii).unwrap();
        let bss = a.get_bytes_list(16);
        assert!(bss.len() > 1);
        let b = Secs2::parse(&bss).unwrap();
        assert_eq!(b.get_ascii().unwrap(), ascii);
    }

    #[test]
    fn secs2_multiblock_roundtrip() {
        // Secs4Net.Tests: secs2-multiblock-roundtrip
        let big = "X".repeat(600);
        let a = Secs2::ascii(&big).unwrap();
        let bss = a.get_bytes_list(100);
        assert!(bss.len() > 1, "should multi-chunk");
        let b = Secs2::parse(&bss).unwrap();
        assert_eq!(b.size(), 600);
        assert_eq!(b.get_ascii().unwrap(), big);
    }

    #[test]
    fn secs2_multiblock_int4() {
        // Secs4Net.Tests: secs2-multiblock-int4
        let vals: Vec<i32> = (0..300).map(|i| i * 7).collect();
        let a = Secs2::int4(vals).unwrap();
        let bss = a.get_bytes_list(128);
        assert!(bss.len() > 1, "int4 body should multi-chunk at max=128");
        let b = Secs2::parse(&bss).unwrap();
        assert_eq!(b.size(), 300);
        assert_eq!(b.get_int_at(&[0]).unwrap(), 0);
        assert_eq!(b.get_int_at(&[299]).unwrap(), 7 * 299);
    }
}
