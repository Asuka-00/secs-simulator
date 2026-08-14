//! SML message / data-item parser (oracle-oriented subset).
//!
//! Source: `SmlMessage.Of` / `AbstractSmlDataItemParser` (type coverage; not full grammar).
//!
//! Supports:
//! - `S{stream}F{func} [W] .`
//! - Nested `<L ... >`
//! - A (quoted + `0xNN` escapes), B (hex/dec), BOOLEAN
//! - U1/U2/U4/U8, I1/I2/I4/I8, F4/F8
//! - Optional size bracket after type: `<U4[1] 100>`

use crate::secs2::Secs2;

use super::error::{Error, Result};

/// Parsed SML message.
#[derive(Debug, Clone, PartialEq)]
pub struct SmlMessage {
    stream: i32,
    function: i32,
    wbit: bool,
    body: Secs2,
}

impl SmlMessage {
    /// `SmlMessage.Of(string)`.
    pub fn of(s: &str) -> Result<Self> {
        let s = s.trim();
        if !s.ends_with('.') {
            return Err(Error::NotFoundEndPeriod);
        }
        let mut p = Parser::new(&s[..s.len() - 1]);
        p.skip_ws();
        p.expect_byte(b'S')?;
        let stream = p.read_uint()?;
        if !(0..=127).contains(&stream) {
            return Err(Error::StreamOutOfRange);
        }
        p.skip_ws();
        p.expect_byte(b'F')?;
        let function = p.read_uint()?;
        if !(0..=255).contains(&function) {
            return Err(Error::FunctionOutOfRange);
        }
        p.skip_ws();
        let mut wbit = false;
        if p.peek().map(|c| c.eq_ignore_ascii_case(&b'w')).unwrap_or(false) {
            p.bump();
            wbit = true;
            p.skip_ws();
        }
        let body = if p.peek() == Some(b'<') {
            p.parse_item()?
        } else {
            Secs2::empty()
        };
        p.skip_ws();
        if !p.eof() {
            return Err(Error::Parse("trailing junk"));
        }
        Ok(Self {
            stream,
            function,
            wbit,
            body,
        })
    }

    pub fn get_stream(&self) -> i32 {
        self.stream
    }

    pub fn get_function(&self) -> i32 {
        self.function
    }

    pub fn wbit(&self) -> bool {
        self.wbit
    }

    pub fn secs2(&self) -> &Secs2 {
        &self.body
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            i: 0,
        }
    }

    fn eof(&self) -> bool {
        self.i >= self.bytes.len()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    fn bump(&mut self) {
        self.i += 1;
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump();
        }
    }

    fn expect_byte(&mut self, b: u8) -> Result<()> {
        if self.peek() == Some(b) {
            self.bump();
            Ok(())
        } else {
            Err(Error::Parse("expected char"))
        }
    }

    fn read_uint(&mut self) -> Result<i32> {
        let start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
        if start == self.i {
            return Err(Error::Parse("expected number"));
        }
        let s = std::str::from_utf8(&self.bytes[start..self.i]).map_err(|_| Error::Parse("utf8"))?;
        s.parse().map_err(|_| Error::Parse("number"))
    }

    fn parse_item(&mut self) -> Result<Secs2> {
        self.skip_ws();
        self.expect_byte(b'<')?;
        self.skip_ws();
        let type_tok = self.read_type_token()?;
        self.skip_size_bracket()?;
        self.skip_ws();
        let item = match type_tok.as_str() {
            "L" => self.parse_list()?,
            "A" => self.parse_ascii()?,
            "B" => self.parse_binary()?,
            "BOOLEAN" => self.parse_boolean()?,
            "U1" => Secs2::uint1(self.parse_u_ints(|n| n as u8)?).map_err(Error::Secs2)?,
            "U2" => Secs2::uint2(self.parse_u_ints(|n| n as u16)?).map_err(Error::Secs2)?,
            "U4" => Secs2::uint4(self.parse_u_ints(|n| n as u32)?).map_err(Error::Secs2)?,
            "U8" => Secs2::uint8(self.parse_u_ints(|n| n)?).map_err(Error::Secs2)?,
            "I1" => Secs2::int1(self.parse_i_ints(|n| n as i8)?).map_err(Error::Secs2)?,
            "I2" => Secs2::int2(self.parse_i_ints(|n| n as i16)?).map_err(Error::Secs2)?,
            "I4" => Secs2::int4(self.parse_i_ints(|n| n as i32)?).map_err(Error::Secs2)?,
            "I8" => Secs2::int8(self.parse_i_ints(|n| n)?).map_err(Error::Secs2)?,
            "F4" => Secs2::float4(self.parse_f32s()?).map_err(Error::Secs2)?,
            "F8" => Secs2::float8(self.parse_f64s()?).map_err(Error::Secs2)?,
            _ => return Err(Error::DataItem("unsupported type")),
        };
        self.skip_ws();
        self.expect_byte(b'>')?;
        Ok(item)
    }

    fn read_type_token(&mut self) -> Result<String> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() {
                self.bump();
            } else {
                break;
            }
        }
        if start == self.i {
            return Err(Error::DataItem("type token"));
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.i])
            .map_err(|_| Error::Parse("utf8"))?
            .to_ascii_uppercase())
    }

    /// Optional `[n]` size after type token (value ignored; parity with SeekSizeString).
    fn skip_size_bracket(&mut self) -> Result<()> {
        self.skip_ws();
        if self.peek() != Some(b'[') {
            return Ok(());
        }
        self.bump();
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
        if self.peek() != Some(b']') {
            return Err(Error::DataItem("size bracket"));
        }
        self.bump();
        Ok(())
    }

    fn parse_list(&mut self) -> Result<Secs2> {
        let mut kids = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'>') => break,
                Some(b'<') => kids.push(self.parse_item()?),
                Some(_) => return Err(Error::DataItem("list child")),
                None => return Err(Error::DataItem("unterminated list")),
            }
        }
        Secs2::list(kids).map_err(Error::Secs2)
    }

    fn parse_ascii(&mut self) -> Result<Secs2> {
        // Accumulate quoted fragments and 0xNN / decimal byte escapes until '>'.
        let mut out = String::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'>') | None => break,
                Some(b'"') => {
                    self.bump();
                    let start = self.i;
                    while let Some(c) = self.peek() {
                        if c == b'"' {
                            let s = std::str::from_utf8(&self.bytes[start..self.i])
                                .map_err(|_| Error::Parse("utf8"))?;
                            out.push_str(s);
                            self.bump();
                            break;
                        }
                        self.bump();
                    }
                }
                Some(b'0') => {
                    let tok = self.read_token_until_ws_or_gt()?;
                    let b = parse_byte_token(&tok)?;
                    out.push(b as char);
                }
                _ => return Err(Error::DataItem("ascii quote or 0x")),
            }
        }
        Secs2::ascii(out).map_err(Error::Secs2)
    }

    fn parse_binary(&mut self) -> Result<Secs2> {
        let mut vals = Vec::new();
        loop {
            self.skip_ws();
            if matches!(self.peek(), Some(b'>') | None) {
                break;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9') | Some(b'-')) {
                break;
            }
            let tok = self.read_token_until_ws_or_gt()?;
            vals.push(parse_byte_token(&tok)?);
        }
        Secs2::binary(vals).map_err(Error::Secs2)
    }

    fn parse_boolean(&mut self) -> Result<Secs2> {
        let mut vals = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'>') | None => break,
                Some(b'T' | b't' | b'F' | b'f' | b'0'..=b'9') => {
                    let tok = self.read_token_until_ws_or_gt()?;
                    let lower = tok.to_ascii_lowercase();
                    if lower == "t" || lower == "true" {
                        vals.push(true);
                    } else if lower == "f" || lower == "false" {
                        vals.push(false);
                    } else {
                        vals.push(parse_byte_token(&tok)? != 0);
                    }
                }
                _ => break,
            }
        }
        Secs2::bool_values(vals).map_err(Error::Secs2)
    }

    fn parse_u_ints<T, F>(&mut self, map: F) -> Result<Vec<T>>
    where
        F: Fn(u64) -> T,
    {
        let mut vals = Vec::new();
        loop {
            self.skip_ws();
            if matches!(self.peek(), Some(b'>') | None) {
                break;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9') | Some(b'-')) {
                break;
            }
            let tok = self.read_token_until_ws_or_gt()?;
            let n = parse_i64_token(&tok)?.unsigned_abs();
            vals.push(map(n));
        }
        Ok(vals)
    }

    fn parse_i_ints<T, F>(&mut self, map: F) -> Result<Vec<T>>
    where
        F: Fn(i64) -> T,
    {
        let mut vals = Vec::new();
        loop {
            self.skip_ws();
            if matches!(self.peek(), Some(b'>') | None) {
                break;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9') | Some(b'-')) {
                break;
            }
            let tok = self.read_token_until_ws_or_gt()?;
            let n = parse_i64_token(&tok)?;
            vals.push(map(n));
        }
        Ok(vals)
    }

    fn parse_f32s(&mut self) -> Result<Vec<f32>> {
        let mut vals = Vec::new();
        loop {
            self.skip_ws();
            if matches!(self.peek(), Some(b'>') | None) {
                break;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9') | Some(b'-') | Some(b'+') | Some(b'.')) {
                break;
            }
            let tok = self.read_token_until_ws_or_gt()?;
            vals.push(
                tok.parse::<f32>()
                    .map_err(|_| Error::DataItem("float4 parse"))?,
            );
        }
        Ok(vals)
    }

    fn parse_f64s(&mut self) -> Result<Vec<f64>> {
        let mut vals = Vec::new();
        loop {
            self.skip_ws();
            if matches!(self.peek(), Some(b'>') | None) {
                break;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9') | Some(b'-') | Some(b'+') | Some(b'.')) {
                break;
            }
            let tok = self.read_token_until_ws_or_gt()?;
            vals.push(
                tok.parse::<f64>()
                    .map_err(|_| Error::DataItem("float8 parse"))?,
            );
        }
        Ok(vals)
    }

    fn read_token_until_ws_or_gt(&mut self) -> Result<String> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'"') {
                break;
            }
            self.bump();
        }
        if start == self.i {
            return Err(Error::DataItem("empty token"));
        }
        std::str::from_utf8(&self.bytes[start..self.i])
            .map(|s| s.to_string())
            .map_err(|_| Error::Parse("utf8"))
    }
}

/// `0xNN` hex or decimal integer → byte (parity with ToByte).
fn parse_byte_token(tok: &str) -> Result<u8> {
    let t = tok.trim();
    if let Some(rest) = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
    {
        u8::from_str_radix(rest, 16).map_err(|_| Error::DataItem("hex byte"))
    } else {
        let n: i64 = t.parse().map_err(|_| Error::DataItem("byte number"))?;
        Ok(n as u8)
    }
}

fn parse_i64_token(tok: &str) -> Result<i64> {
    let t = tok.trim();
    if let Some(rest) = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
    {
        i64::from_str_radix(rest, 16).map_err(|_| Error::DataItem("hex int"))
    } else {
        t.parse().map_err(|_| Error::DataItem("int parse"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sml_header_only() {
        let m = SmlMessage::of("S1F1 W.").unwrap();
        assert_eq!(m.get_stream(), 1);
        assert_eq!(m.get_function(), 1);
        assert!(m.wbit());
    }

    #[test]
    fn sml_with_body() {
        let m = SmlMessage::of("S6F11 W <L <U4 100> <A \"OK\"> >.").unwrap();
        assert_eq!(m.get_stream(), 6);
        assert_eq!(m.get_function(), 11);
        assert!(m.wbit());
        let body = m.secs2();
        assert_eq!(body.size(), 2);
        assert_eq!(body.get_int_at(&[0, 0]).unwrap(), 100);
        assert_eq!(body.get_ascii_at(&[1]).unwrap(), "OK");
    }

    #[test]
    fn sml_numeric_types_and_size_bracket() {
        let m = SmlMessage::of(
            r#"S2F0 <L
                <U1[2] 1 2>
                <U2 300>
                <I2 -5 6>
                <I8 9000000000>
                <U8 4000000000>
                <F4 1.5 -2.25>
                <F8 3.125>
            >."#,
        )
        .unwrap();
        let b = m.secs2();
        assert_eq!(b.get_long_at(&[0, 0]).unwrap(), 1);
        assert_eq!(b.get_long_at(&[0, 1]).unwrap(), 2);
        assert_eq!(b.get_long_at(&[1, 0]).unwrap(), 300);
        assert_eq!(b.get_long_at(&[2, 0]).unwrap(), -5);
        assert_eq!(b.get_long_at(&[2, 1]).unwrap(), 6);
        assert_eq!(b.get_long_at(&[3, 0]).unwrap(), 9_000_000_000);
        assert_eq!(b.get_long_at(&[4, 0]).unwrap(), 4_000_000_000);
        assert_eq!(b.get_float_at(&[5, 0]).unwrap(), 1.5);
        assert_eq!(b.get_float_at(&[5, 1]).unwrap(), -2.25);
        assert!((b.get_double_at(&[6, 0]).unwrap() - 3.125).abs() < 1e-12);
    }

    #[test]
    fn sml_binary_hex_and_boolean() {
        let m = SmlMessage::of(r#"S1F0 <L <B 0x01 0xFF 10> <BOOLEAN T F true false 0x01 0x00> >."#)
            .unwrap();
        let b = m.secs2();
        assert_eq!(b.size(), 2);
        assert_eq!(b.get_item(&[0]).unwrap().size(), 3);
        assert!(b.get_boolean_at(&[1, 0]).unwrap());
        assert!(!b.get_boolean_at(&[1, 1]).unwrap());
        assert!(b.get_boolean_at(&[1, 2]).unwrap());
        assert!(!b.get_boolean_at(&[1, 3]).unwrap());
        assert!(b.get_boolean_at(&[1, 4]).unwrap());
        assert!(!b.get_boolean_at(&[1, 5]).unwrap());
    }

    #[test]
    fn sml_ascii_hex_escape() {
        // 0x41 = 'A'
        let m = SmlMessage::of(r#"S1F0 <A "X" 0x41 "Y">."#).unwrap();
        assert_eq!(m.secs2().get_ascii().unwrap(), "XAY");
    }

    #[test]
    fn sml_error_stream_function_period() {
        assert_eq!(
            SmlMessage::of("S128F1.").unwrap_err(),
            Error::StreamOutOfRange
        );
        assert_eq!(
            SmlMessage::of("S1F256.").unwrap_err(),
            Error::FunctionOutOfRange
        );
        assert_eq!(
            SmlMessage::of("S1F1 W").unwrap_err(),
            Error::NotFoundEndPeriod
        );
    }

    #[test]
    fn sml_empty_list_and_no_wbit() {
        let m = SmlMessage::of("S5F1 <L>.").unwrap();
        assert!(!m.wbit());
        assert_eq!(m.secs2().size(), 0);
    }
}
