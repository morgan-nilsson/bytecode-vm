use thiserror::Error;

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    #[error("Unexpected end of input at position {at}, wanted {wanted} bytes")]
    Truncated { at: usize, wanted: usize },
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    /// A reader over a sub-slice, for parsing a structure whose length is
    /// known up front (an attribute body, say) without letting it read past.
    pub fn sub(&mut self, n: usize) -> Result<Reader<'a>, ParseError> {
        Ok(Reader::new(self.bytes(n)?))
    }

    pub fn u8(&mut self) -> Result<u8, ParseError> {
        let b = self.bytes(1)?;
        Ok(b[0])
    }
    pub fn u16(&mut self) -> Result<u16, ParseError> {
        let b = self.bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    pub fn u32(&mut self) -> Result<u32, ParseError> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn u64(&mut self) -> Result<u64, ParseError> {
        let b = self.bytes(8)?;
        Ok(u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    pub fn i32(&mut self) -> Result<i32, ParseError> {
        Ok(self.u32()? as i32)
    }
    pub fn i64(&mut self) -> Result<i64, ParseError> {
        Ok(self.u64()? as i64)
    }

    /// `from_bits` keeps the exact bit pattern, NaN payloads included.
    pub fn f32(&mut self) -> Result<f32, ParseError> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub fn f64(&mut self) -> Result<f64, ParseError> {
        Ok(f64::from_bits(self.u64()?))
    }

    /// Borrow `n` bytes — the thing Read can't do.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or(ParseError::Truncated { at: self.pos, wanted: n })?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Bytes not yet read. A class file must end with none left over (JVMS 4.8).
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }
}
