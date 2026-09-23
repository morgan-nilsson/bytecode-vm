//! The byte reader: big-endian reads, borrowing, and truncation detection.
//! Ported from `c_backup/tests/test_classfile_io.c`.

use bytecode_vm::parser::reader::{ParseError, Reader};

#[test]
fn u16_is_big_endian() {
    let mut r = Reader::new(&[0x12, 0x34]);
    assert_eq!(r.u16().unwrap(), 0x1234);
    assert_eq!(r.pos(), 2);
}

#[test]
fn u16_high_bit_values() {
    let mut r = Reader::new(&[0xff, 0xfe, 0x80, 0x00, 0x00, 0x00]);
    assert_eq!(r.u16().unwrap(), 0xfffe);
    assert_eq!(r.u16().unwrap(), 0x8000);
    assert_eq!(r.u16().unwrap(), 0x0000);
}

#[test]
fn u32_is_big_endian() {
    let mut r = Reader::new(&[0xca, 0xfe, 0xba, 0xbe]);
    assert_eq!(r.u32().unwrap(), 0xcafe_babe);
    assert_eq!(r.pos(), 4);
}

#[test]
fn u32_each_byte_position() {
    let mut r = Reader::new(&[
        0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x01,
    ]);
    assert_eq!(r.u32().unwrap(), 0x0100_0000);
    assert_eq!(r.u32().unwrap(), 0x0001_0000);
    assert_eq!(r.u32().unwrap(), 0x0000_0100);
    assert_eq!(r.u32().unwrap(), 0x0000_0001);
}

#[test]
fn u32_max_value() {
    let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff]);
    assert_eq!(r.u32().unwrap(), u32::MAX);
}

#[test]
fn u64_high_bytes_come_first() {
    // Catches a swapped pair of 32-bit reads, which C made easy to get wrong.
    let mut r = Reader::new(&[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]);
    assert_eq!(r.u64().unwrap(), 0x0123_4567_89AB_CDEF);
}

#[test]
fn signed_reads_wrap_to_negative() {
    let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff]);
    assert_eq!(r.i32().unwrap(), -1);

    let mut r = Reader::new(&[0x80, 0x00, 0x00, 0x00]);
    assert_eq!(r.i32().unwrap(), i32::MIN);

    let mut r = Reader::new(&[0x80, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(r.i64().unwrap(), i64::MIN);
}

#[test]
fn float_keeps_the_exact_bit_pattern() {
    let mut r = Reader::new(&[0x40, 0x49, 0x0F, 0xDB]);
    let f = r.f32().unwrap();
    assert_eq!(f.to_bits(), 0x4049_0FDB);
    assert!(f > 3.14159 && f < 3.14160);
}

#[test]
fn float_special_values() {
    for (bytes, check) in [
        ([0x00u8, 0x00, 0x00, 0x00], "zero"),
        ([0x80, 0x00, 0x00, 0x00], "negative zero"),
        ([0x7F, 0x80, 0x00, 0x00], "infinity"),
        ([0xFF, 0x80, 0x00, 0x00], "negative infinity"),
        ([0x7F, 0xC0, 0x00, 0x00], "nan"),
    ] {
        let mut r = Reader::new(&bytes);
        let f = r.f32().unwrap();
        let expected = u32::from_be_bytes(bytes);
        assert_eq!(f.to_bits(), expected, "{check} lost its bit pattern");
    }
}

#[test]
fn double_keeps_the_exact_bit_pattern() {
    let mut r = Reader::new(&[0x40, 0x09, 0x21, 0xFB, 0x54, 0x44, 0x2D, 0x18]);
    let d = r.f64().unwrap();
    assert_eq!(d.to_bits(), 0x4009_21FB_5444_2D18);
    assert_eq!(d, std::f64::consts::PI);
}

#[test]
fn double_nan_payload_survives() {
    // A NaN with a non-canonical payload must come back bit-for-bit.
    let bits: u64 = 0x7FF8_0000_DEAD_BEEF;
    let bytes = bits.to_be_bytes();
    let mut r = Reader::new(&bytes);
    let d = r.f64().unwrap();
    assert!(d.is_nan());
    assert_eq!(d.to_bits(), bits);
}

#[test]
fn reads_advance_sequentially() {
    let mut r = Reader::new(&[0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x03]);
    assert_eq!(r.u16().unwrap(), 1);
    assert_eq!(r.u32().unwrap(), 2);
    assert_eq!(r.u16().unwrap(), 3);
    assert_eq!(r.pos(), 8);
    assert!(r.is_empty());
}

#[test]
fn bytes_borrows_from_the_input() {
    let buf = [1u8, 2, 3, 4, 5];
    let mut r = Reader::new(&buf);
    let slice = r.bytes(3).unwrap();
    assert_eq!(slice, &[1, 2, 3]);
    assert_eq!(r.pos(), 3);
    // The borrow is of the original buffer, not a copy.
    assert!(std::ptr::eq(slice.as_ptr(), buf.as_ptr()));
}

#[test]
fn short_read_is_truncated_not_silent() {
    let mut r = Reader::new(&[0x12]);
    assert_eq!(r.u16(), Err(ParseError::Truncated { at: 0, wanted: 2 }));
    // A failed read must not advance the position.
    assert_eq!(r.pos(), 0);
}

#[test]
fn every_read_kind_detects_truncation() {
    let buf = [0u8; 3];
    for wanted in 4..=8 {
        let mut r = Reader::new(&buf);
        assert!(r.bytes(wanted).is_err(), "bytes({wanted}) should fail with 3 available");
    }
    assert!(Reader::new(&buf).u32().is_err());
    assert!(Reader::new(&buf).u64().is_err());
    assert!(Reader::new(&[]).u8().is_err());
    assert!(Reader::new(&[]).u16().is_err());
}

#[test]
fn reading_the_last_byte_leaves_nothing() {
    let mut r = Reader::new(&[0xff]);
    assert_eq!(r.u8().unwrap(), 0xff);
    assert!(r.is_empty());
    assert_eq!(r.remaining(), 0);
    assert!(r.u8().is_err());
}

#[test]
fn remaining_counts_down() {
    let mut r = Reader::new(&[0, 1, 2, 3, 4, 5]);
    assert_eq!(r.remaining(), 6);
    r.u16().unwrap();
    assert_eq!(r.remaining(), 4);
    r.bytes(4).unwrap();
    assert_eq!(r.remaining(), 0);
}

#[test]
fn huge_length_does_not_overflow() {
    // A u32 length from a malformed file must not wrap when added to pos.
    let mut r = Reader::new(&[1, 2, 3, 4]);
    assert!(r.bytes(usize::MAX).is_err());
    assert_eq!(r.pos(), 0);
}

#[test]
fn sub_reader_is_bounded_to_its_slice() {
    let buf = [0xAAu8, 0xBB, 0xCC, 0xDD, 0xEE];
    let mut r = Reader::new(&buf);
    let mut inner = r.sub(2).unwrap();
    assert_eq!(inner.u8().unwrap(), 0xAA);
    assert_eq!(inner.u8().unwrap(), 0xBB);
    // The sub-reader cannot see past its own slice.
    assert!(inner.u8().is_err());
    assert!(inner.is_empty());
    // The outer reader carried on from where the slice ended.
    assert_eq!(r.pos(), 2);
    assert_eq!(r.u8().unwrap(), 0xCC);
}

#[test]
fn sub_reader_longer_than_the_input_fails() {
    let mut r = Reader::new(&[1, 2]);
    assert!(r.sub(3).is_err());
}

#[test]
fn empty_input() {
    let r = Reader::new(&[]);
    assert!(r.is_empty());
    assert_eq!(r.remaining(), 0);
    assert_eq!(r.pos(), 0);
}
