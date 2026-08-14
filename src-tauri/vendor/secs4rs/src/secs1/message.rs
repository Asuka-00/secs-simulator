//! SECS-I data message: header fields + blocks + SECS-II body.
//!
//! Source: `Secs1ValidMessage` / `AbstractSecs1MessageBuilder`.

use crate::secs2::Secs2;

use super::block::Secs1MessageBlock;
use super::error::{Error, Result};

const HEADER_SIZE: usize = 10;

/// SECS-I message (valid path used by Batch5 oracle).
#[derive(Debug, Clone, PartialEq)]
pub struct Secs1Message {
    header: [u8; HEADER_SIZE],
    body: Secs2,
    blocks: Vec<Secs1MessageBlock>,
    stream: i32,
    function: i32,
    wbit: bool,
    device_id: i32,
    rbit: bool,
    valid_blocks: bool,
}

impl Secs1Message {
    /// Build data message: split SECS-II wire into ≤244-byte blocks.
    ///
    /// `Secs1MessageBuilder.BuildDataMessage(header, body)`.
    pub fn build_data_message(header: &[u8], body: Secs2) -> Result<Self> {
        if header.len() != HEADER_SIZE {
            return Err(Error::HeaderByteLength);
        }
        let mut h = [0u8; HEADER_SIZE];
        h.copy_from_slice(header);

        let chunks = body.get_bytes_list(244);
        if chunks.len() > 0x7FFE {
            return Err(Error::TooBigMessageBody);
        }

        // Empty wire still yields one empty trailing chunk from get_bytes_list.
        let mut blocks = Vec::with_capacity(chunks.len().max(1));
        let mut block_num = Secs1MessageBlock::ONE;
        let m = chunks.len().saturating_sub(1);

        for (i, chunk) in chunks.iter().enumerate() {
            let ebit = i == m;
            blocks.push(build_block(&h, chunk, ebit, block_num));
            block_num += 1;
        }

        Ok(Self::from_parts(h, body, blocks, true))
    }

    /// Reassemble from blocks (`BuildFromBlocks`). Invalid sequence → `Error::InvalidBlocks`.
    pub fn build_from_blocks(blocks: &[Secs1MessageBlock]) -> Result<Self> {
        if blocks.is_empty() {
            return Err(Error::EmptyBlockList);
        }
        if !is_valid_blocks(blocks) {
            return Err(Error::InvalidBlocks);
        }

        let mut bss: Vec<Vec<u8>> = Vec::new();
        let mut buf = &blocks[0];
        bss.push(buf.body_bytes().unwrap_or(&[]).to_vec());

        for block in blocks.iter().skip(1) {
            if buf.is_next_block(block) {
                bss.push(block.body_bytes().unwrap_or(&[]).to_vec());
                buf = block;
            }
        }

        let body = Secs2::parse(&bss).map_err(Error::Secs2)?;
        let last = blocks.last().unwrap().get_bytes();
        if last.len() < 11 {
            return Err(Error::InvalidBlocks);
        }
        let mut h = [0u8; HEADER_SIZE];
        h.copy_from_slice(&last[1..11]);
        Ok(Self::from_parts(h, body, blocks.to_vec(), true))
    }

    fn from_parts(
        header: [u8; HEADER_SIZE],
        body: Secs2,
        blocks: Vec<Secs1MessageBlock>,
        valid_blocks: bool,
    ) -> Self {
        let stream = i32::from(header[2]) & 0x7F;
        let function = i32::from(header[3]) & 0xFF;
        let wbit = (i32::from(header[2]) & 0x80) == 0x80;
        let device_id =
            ((i32::from(header[0]) << 8) & 0x0000_7F00) | (i32::from(header[1]) & 0xFF);
        let rbit = (i32::from(header[0]) & 0x80) == 0x80;
        Self {
            header,
            body,
            blocks,
            stream,
            function,
            wbit,
            device_id,
            rbit,
            valid_blocks,
        }
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

    pub fn device_id(&self) -> i32 {
        self.device_id
    }

    pub fn rbit(&self) -> bool {
        self.rbit
    }

    pub fn secs2(&self) -> &Secs2 {
        &self.body
    }

    pub fn header10_bytes(&self) -> [u8; HEADER_SIZE] {
        self.header
    }

    pub fn to_blocks(&self) -> &[Secs1MessageBlock] {
        &self.blocks
    }

    pub fn is_valid_blocks(&self) -> bool {
        self.valid_blocks
    }

    /// Session-ID equals Device-ID for SECS-I (`AbstractSecs1Message.SessionId`).
    pub fn session_id(&self) -> i32 {
        self.device_id
    }
}

impl crate::SecsMessage for Secs1Message {
    fn get_stream(&self) -> i32 {
        self.stream
    }

    fn get_function(&self) -> i32 {
        self.function
    }

    fn wbit(&self) -> bool {
        self.wbit
    }

    fn secs2(&self) -> &Secs2 {
        &self.body
    }

    fn device_id(&self) -> i32 {
        self.device_id
    }

    fn session_id(&self) -> i32 {
        self.device_id
    }

    fn header10_bytes(&self) -> [u8; 10] {
        self.header
    }
}

/// Assemble one block: length | 10-byte hdr | body | 2-byte checksum.
fn build_block(
    header: &[u8; HEADER_SIZE],
    body: &[u8],
    ebit: bool,
    block_number: i32,
) -> Secs1MessageBlock {
    let len = HEADER_SIZE + body.len();
    let mut bs = vec![0u8; len + 3];
    bs[0] = len as u8;
    bs[1] = header[0];
    bs[2] = header[1];
    bs[3] = header[2];
    bs[4] = header[3];

    bs[5] = (block_number >> 8) as u8;
    if ebit {
        bs[5] |= 0x80;
    }
    bs[6] = block_number as u8;

    bs[7] = header[6];
    bs[8] = header[7];
    bs[9] = header[8];
    bs[10] = header[9];

    let mut sum: i32 = 0;
    let mut pos = 1usize;
    while pos < 11 {
        sum += i32::from(bs[pos]) & 0xFF;
        pos += 1;
    }
    for &b in body {
        bs[pos] = b;
        sum += i32::from(b) & 0xFF;
        pos += 1;
    }
    bs[pos] = (sum >> 8) as u8;
    bs[pos + 1] = sum as u8;

    Secs1MessageBlock::of(bs)
}

/// Validate block sequence (subset of C# IsValidBlocks used by BuildFromBlocks).
fn is_valid_blocks(blocks: &[Secs1MessageBlock]) -> bool {
    if blocks.is_empty() {
        return false;
    }
    for b in blocks {
        if !b.is_valid() {
            return false;
        }
    }
    if !blocks[0].is_first_block() {
        return false;
    }
    let m = blocks.len();
    if !blocks[m - 1].ebit() {
        return false;
    }

    let first = &blocks[0];
    let ref_bytes = first.get_bytes();
    let mut ebit_count = if first.ebit() { 1 } else { 0 };
    let mut buf = first;

    for block in blocks.iter().skip(1) {
        if !buf.equals_system_bytes(block) {
            return false;
        }
        let bs = block.get_bytes();
        if !buf.is_next_block(block) && buf.block_number() != block.block_number() {
            return false;
        }
        if ref_bytes[1] != bs[1]
            || ref_bytes[2] != bs[2]
            || ref_bytes[3] != bs[3]
            || ref_bytes[4] != bs[4]
        {
            return false;
        }
        if block.ebit() {
            ebit_count += 1;
            if ebit_count > 1 {
                return false;
            }
        }
        buf = block;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secs2::Secs2;

    fn header() -> [u8; 10] {
        // device=0x0005, byte[2]=0x81 (wbit+stream1), func=1, sys=AA BB CC DD
        [0x00, 0x05, 0x81, 0x01, 0x00, 0x00, 0xAA, 0xBB, 0xCC, 0xDD]
    }

    #[test]
    fn secs1_header_fields() {
        let m = Secs1Message::build_data_message(&header(), Secs2::ascii("HI").unwrap()).unwrap();
        assert_eq!(m.get_stream(), 1);
        assert_eq!(m.get_function(), 1);
        assert!(m.wbit());
        assert_eq!(m.device_id(), 5);
    }

    #[test]
    fn secs1_implements_secs_message() {
        // AbstractSecs1Message.SessionId() => DeviceId()
        use crate::SecsMessage as _;
        let m = Secs1Message::build_data_message(&header(), Secs2::ascii("HI").unwrap()).unwrap();
        assert_eq!(m.session_id(), 5);
        assert_eq!(m.session_id(), m.device_id());
        assert_eq!(m.get_stream(), 1);
        assert_eq!(m.get_function(), 1);
        assert!(m.wbit());
        assert_eq!(m.secs2().get_ascii().unwrap(), "HI");
        assert_eq!(m.header10_bytes(), header());
    }

    #[test]
    fn secs1_single_block() {
        let m =
            Secs1Message::build_data_message(&header(), Secs2::ascii("HELLO").unwrap()).unwrap();
        let blocks = m.to_blocks();
        assert_eq!(blocks.len(), 1);
        let b0 = &blocks[0];
        assert!(b0.ebit(), "单块应 Ebit=true");
        assert_eq!(b0.block_number(), 1);
        assert_eq!(b0.device_id(), 5);
        assert!(b0.check_sum());
        assert!(m.is_valid_blocks());
    }

    #[test]
    fn secs1_multi_block() {
        let body = Secs2::ascii("Z".repeat(500)).unwrap();
        let m = Secs1Message::build_data_message(&header(), body).unwrap();
        let blocks = m.to_blocks();
        assert!(blocks.len() >= 2, "应多块,实际 {}", blocks.len());
        for (i, b) in blocks.iter().enumerate() {
            assert_eq!(b.block_number(), (i + 1) as i32);
            let is_last = i + 1 == blocks.len();
            assert_eq!(b.ebit(), is_last);
            assert!(b.check_sum());
        }
        assert!(m.is_valid_blocks());
    }

    #[test]
    fn secs1_roundtrip_from_blocks() {
        let body = Secs2::list([
            Secs2::ascii("ABC").unwrap(),
            Secs2::int4([7, 8, 9]).unwrap(),
        ])
        .unwrap();
        let m = Secs1Message::build_data_message(&header(), body).unwrap();
        let m2 = Secs1Message::build_from_blocks(m.to_blocks()).unwrap();
        assert_eq!(m2.get_stream(), 1);
        assert_eq!(m2.get_function(), 1);
        assert!(m2.wbit());
        assert_eq!(m2.secs2().get_ascii_at(&[0]).unwrap(), "ABC");
        assert_eq!(m2.secs2().get_int_at(&[1, 1]).unwrap(), 8);
    }

    #[test]
    fn secs1_multiblock_roundtrip() {
        let big = "Q".repeat(700);
        let m = Secs1Message::build_data_message(&header(), Secs2::ascii(&big).unwrap()).unwrap();
        let m2 = Secs1Message::build_from_blocks(m.to_blocks()).unwrap();
        assert_eq!(m2.secs2().get_ascii().unwrap(), big);
    }
}
