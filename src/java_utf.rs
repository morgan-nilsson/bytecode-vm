//! Modified UTF-8, the encoding used by CONSTANT_Utf8 (JVMS 4.4.7).
//!
//! It differs from standard UTF-8 in two ways:
//!   * U+0000 is encoded as the two bytes `C0 80`, so a string never contains
//!     a zero byte.
//!   * Characters above U+FFFF are encoded as a surrogate pair, each half in
//!     three bytes, for six bytes total rather than the standard four.
//!
//! Neither form is valid UTF-8, so these bytes cannot be held in a Rust `str`.
//! `JavaUTF8` keeps the raw bytes and decodes on demand.

use core::fmt;
use std::fmt::Write;
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum JavaUTF8Error {
    #[error("Invalid Java UTF-8 sequence")]
    InvalidSequence,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct JavaUTF8<'a>(pub &'a [u8]);

impl<'a> JavaUTF8<'a> {
    pub fn as_bytes(&self) -> &'a [u8] {
        self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Checks the bytes against the rules in JVMS 4.4.7: no byte may be zero
    /// or lie in `0xF0..=0xFF`, and every multi-byte sequence must be complete.
    ///
    /// This is deliberately byte-level only. A CONSTANT_Utf8 encodes a sequence
    /// of UTF-16 code units, and an unpaired surrogate is one of those — javac
    /// emits `ED A0 BD` for the string `"a\uD83Db"`. Such a string is a legal
    /// constant even though it cannot be turned into a Rust `String`; that is
    /// `to_string_checked`'s problem, not this one.
    pub fn validate(&self) -> Result<(), JavaUTF8Error> {
        let bytes = self.0;
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            let width = match b {
                0x00 => return Err(JavaUTF8Error::InvalidSequence),
                0x01..=0x7F => 1,
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                // 0x80..=0xBF as a leader, and 0xF0..=0xFF, which JVMS forbids.
                _ => return Err(JavaUTF8Error::InvalidSequence),
            };
            if i + width > bytes.len() {
                return Err(JavaUTF8Error::InvalidSequence);
            }
            if bytes[i + 1..i + width].iter().any(|c| c & 0xC0 != 0x80) {
                return Err(JavaUTF8Error::InvalidSequence);
            }
            i += width;
        }
        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    /// Decodes to a `String`. Fails on a malformed sequence and on an unpaired
    /// surrogate, which `validate` accepts but Rust cannot represent.
    pub fn to_string_checked(&self) -> Result<String, JavaUTF8Error> {
        decode_java_utf8(self.0).into_iter().collect()
    }
}

/// Comparing against a `str` compares the raw bytes, so `name == "Code"` works
/// for the common ASCII case without decoding.
impl PartialEq<str> for JavaUTF8<'_> {
    fn eq(&self, other: &str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<&str> for JavaUTF8<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<[u8]> for JavaUTF8<'_> {
    fn eq(&self, other: &[u8]) -> bool {
        self.0 == other
    }
}

impl AsRef<[u8]> for JavaUTF8<'_> {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

impl<'a> From<&'a [u8]> for JavaUTF8<'a> {
    fn from(bytes: &'a [u8]) -> Self {
        JavaUTF8(bytes)
    }
}

impl<'a> From<&'a str> for JavaUTF8<'a> {
    fn from(s: &'a str) -> Self {
        JavaUTF8(s.as_bytes())
    }
}

pub fn decode_java_utf8(bytes: &[u8]) -> Vec<Result<char, JavaUTF8Error>> {
    let mut chars = Vec::new();
    let mut i = 0;

    // Reads the 3-byte group at `at`, returning its raw value without
    // rejecting surrogates, so a pair can be recombined below.
    fn three_byte_group(bytes: &[u8], at: usize) -> Option<u32> {
        let (b0, b1, b2) = (*bytes.get(at)?, *bytes.get(at + 1)?, *bytes.get(at + 2)?);
        if b0 & 0xF0 != 0xE0 || b1 & 0xC0 != 0x80 || b2 & 0xC0 != 0x80 {
            return None;
        }
        Some(((b0 & 0x0F) as u32) << 12 | ((b1 & 0x3F) as u32) << 6 | (b2 & 0x3F) as u32)
    }

    while i < bytes.len() {
        let byte = bytes[i];

        if byte == 0x00 {
            // U+0000 must be written as C0 80; a raw zero byte is not allowed.
            chars.push(Err(JavaUTF8Error::InvalidSequence));
            break;
        } else if byte & 0x80 == 0 {
            // 1-byte sequence (ASCII)
            chars.push(Ok(byte as char));
            i += 1;
        } else if byte & 0xE0 == 0xC0 {
            // 2-byte sequence
            if i + 1 >= bytes.len() {
                chars.push(Err(JavaUTF8Error::InvalidSequence));
                break;
            }
            let b1 = bytes[i + 1];
            if b1 & 0xC0 != 0x80 {
                chars.push(Err(JavaUTF8Error::InvalidSequence));
                break;
            }
            let code_point = ((byte & 0x1F) as u32) << 6 | (b1 & 0x3F) as u32;
            // C0 80 is the encoding of U+0000 and is the one legal overlong form.
            chars.push(std::char::from_u32(code_point).ok_or(JavaUTF8Error::InvalidSequence));
            i += 2;
        } else if byte & 0xF0 == 0xE0 {
            // 3-byte sequence, or the first half of a surrogate pair.
            let Some(first) = three_byte_group(bytes, i) else {
                chars.push(Err(JavaUTF8Error::InvalidSequence));
                break;
            };

            if (0xD800..=0xDBFF).contains(&first) {
                // High surrogate: the low half must follow, and together they
                // encode one character above U+FFFF.
                match three_byte_group(bytes, i + 3) {
                    Some(second) if (0xDC00..=0xDFFF).contains(&second) => {
                        let code_point =
                            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                        chars.push(
                            std::char::from_u32(code_point)
                                .ok_or(JavaUTF8Error::InvalidSequence),
                        );
                        i += 6;
                    }
                    _ => {
                        chars.push(Err(JavaUTF8Error::InvalidSequence));
                        break;
                    }
                }
            } else if (0xDC00..=0xDFFF).contains(&first) {
                // A low surrogate with no high surrogate before it.
                chars.push(Err(JavaUTF8Error::InvalidSequence));
                break;
            } else {
                chars.push(std::char::from_u32(first).ok_or(JavaUTF8Error::InvalidSequence));
                i += 3;
            }
        } else {
            // 0x80..=0xBF as a leading byte, or 0xF0..=0xFF, which JVMS forbids.
            chars.push(Err(JavaUTF8Error::InvalidSequence));
            break;
        }
    }

    chars
}

impl fmt::Display for JavaUTF8<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match std::str::from_utf8(self.0) {
            Ok(s) => f.write_str(s), // ~always
            Err(_) => {
                for c in decode_java_utf8(self.0) {
                    f.write_char(c.unwrap_or('\u{FFFD}'))?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Debug for JavaUTF8<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\"", self)
    }
}
