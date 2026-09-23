//! Modified UTF-8 (JVMS 4.4.7): the encoding CONSTANT_Utf8 uses, which differs
//! from standard UTF-8 for U+0000 and for characters above U+FFFF.

use bytecode_vm::java_utf::{decode_java_utf8, JavaUTF8, JavaUTF8Error};

fn decode(bytes: &[u8]) -> Result<String, JavaUTF8Error> {
    decode_java_utf8(bytes).into_iter().collect()
}

// ---------------------------------------------------------------------------
// Sequences that are the same as standard UTF-8
// ---------------------------------------------------------------------------

#[test]
fn ascii() {
    assert_eq!(decode(b"hello").unwrap(), "hello");
    assert_eq!(decode(b"").unwrap(), "");
    assert_eq!(decode(b"java/lang/Object").unwrap(), "java/lang/Object");
}

#[test]
fn every_ascii_byte_except_zero() {
    let bytes: Vec<u8> = (1u8..=0x7f).collect();
    let decoded = decode(&bytes).unwrap();
    assert_eq!(decoded.chars().count(), 0x7f);
    assert_eq!(decoded.as_bytes(), &bytes[..]);
}

#[test]
fn two_byte_sequence() {
    // U+00E9 'é'
    assert_eq!(decode(&[0xC3, 0xA9]).unwrap(), "é");
}

#[test]
fn three_byte_sequence() {
    // U+20AC '€'
    assert_eq!(decode(&[0xE2, 0x82, 0xAC]).unwrap(), "€");
}

#[test]
fn boundary_code_points() {
    assert_eq!(decode(&[0xC2, 0x80]).unwrap(), "\u{80}");
    assert_eq!(decode(&[0xDF, 0xBF]).unwrap(), "\u{7FF}");
    assert_eq!(decode(&[0xE0, 0xA0, 0x80]).unwrap(), "\u{800}");
    assert_eq!(decode(&[0xEF, 0xBF, 0xBF]).unwrap(), "\u{FFFF}");
}

// ---------------------------------------------------------------------------
// Where modified UTF-8 differs
// ---------------------------------------------------------------------------

#[test]
fn nul_is_encoded_as_two_bytes() {
    // U+0000 is written C0 80 so a string never contains a zero byte.
    assert_eq!(decode(&[0xC0, 0x80]).unwrap(), "\u{0}");
    assert_eq!(decode(&[b'a', 0xC0, 0x80, b'b']).unwrap(), "a\u{0}b");
}

#[test]
fn raw_zero_byte_is_rejected() {
    // "No byte may have the value (byte)0."
    assert!(decode(&[b'a', 0x00, b'b']).is_err());
    assert!(decode(&[0x00]).is_err());
}

#[test]
fn supplementary_character_as_a_surrogate_pair() {
    // U+1F600 is stored as two 3-byte halves, not the standard 4-byte form.
    let bytes = [0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80];
    assert_eq!(decode(&bytes).unwrap(), "\u{1F600}");
}

#[test]
fn supplementary_character_in_context() {
    let bytes = [b'a', 0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80, b'b'];
    assert_eq!(decode(&bytes).unwrap(), "a\u{1F600}b");
}

#[test]
fn surrogate_pair_boundaries() {
    // U+10000, the lowest supplementary character.
    assert_eq!(decode(&[0xED, 0xA0, 0x80, 0xED, 0xB0, 0x80]).unwrap(), "\u{10000}");
    // U+10FFFF, the highest.
    assert_eq!(decode(&[0xED, 0xAF, 0xBF, 0xED, 0xBF, 0xBF]).unwrap(), "\u{10FFFF}");
}

#[test]
fn unpaired_surrogates_are_valid_bytes_but_do_not_decode() {
    // A CONSTANT_Utf8 holds UTF-16 code units, and an unpaired surrogate is
    // one: javac emits ED A0 BD for the string "a\uD83Db". So the bytes are
    // legal, but Rust has no `char` for them.
    for bytes in [&[0xED, 0xA0, 0xBD][..], &[0xED, 0xA0, 0xBD, b'a'], &[0xED, 0xB8, 0x80]] {
        assert!(JavaUTF8(bytes).is_valid(), "{bytes:02x?} should be valid bytes");
        assert!(decode(bytes).is_err(), "{bytes:02x?} should not decode to a String");
    }
}

#[test]
fn high_surrogate_followed_by_a_non_surrogate_does_not_decode() {
    // The second half must be a low surrogate, not just any 3-byte sequence.
    let bytes = [0xED, 0xA0, 0xBD, 0xE2, 0x82, 0xAC];
    assert!(JavaUTF8(&bytes).is_valid());
    assert!(decode(&bytes).is_err());
}

#[test]
fn standard_four_byte_utf8_is_rejected() {
    // F0 9F 98 80 is how standard UTF-8 writes U+1F600; modified UTF-8 forbids
    // every byte from 0xF0 up.
    assert!(decode(&[0xF0, 0x9F, 0x98, 0x80]).is_err());
}

#[test]
fn bytes_f0_through_ff_are_rejected() {
    for bad in 0xF0u8..=0xFF {
        assert!(decode(&[b'a', bad, b'b']).is_err(), "byte {bad:#04x} was accepted");
    }
}

// ---------------------------------------------------------------------------
// Malformed input
// ---------------------------------------------------------------------------

#[test]
fn lone_continuation_byte_is_rejected() {
    for bad in 0x80u8..=0xBF {
        assert!(decode(&[bad]).is_err(), "byte {bad:#04x} was accepted as a leader");
    }
}

#[test]
fn truncated_multibyte_sequences_are_rejected() {
    assert!(decode(&[0xC3]).is_err());
    assert!(decode(&[0xE2, 0x82]).is_err());
    assert!(decode(&[b'a', 0xE2, 0x82]).is_err());
}

#[test]
fn continuation_byte_with_the_wrong_top_bits_is_rejected() {
    assert!(decode(&[0xC3, 0x28]).is_err());
    assert!(decode(&[0xE2, 0x28, 0xAC]).is_err());
    assert!(decode(&[0xE2, 0x82, 0x28]).is_err());
}

// ---------------------------------------------------------------------------
// JavaUTF8
// ---------------------------------------------------------------------------

#[test]
fn validate_accepts_well_formed_strings() {
    for s in [
        &b"hello"[..],
        &[0xC3, 0xA9],
        &[0xE2, 0x82, 0xAC],
        &[0xC0, 0x80],
        &[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80],
        &[0xED, 0xA0, 0xBD], // an unpaired surrogate is still legal bytes
        &[],
    ] {
        assert!(JavaUTF8(s).is_valid(), "{s:02x?} should be valid");
    }
}

#[test]
fn validate_rejects_malformed_strings() {
    for s in [&[0x00u8][..], &[0xF0, 0x9F, 0x98, 0x80], &[0x80], &[0xE2, 0x82], &[0xC3, 0x28]] {
        assert!(!JavaUTF8(s).is_valid(), "{s:02x?} should be rejected");
    }
}

#[test]
fn len_is_in_bytes_not_characters() {
    let euro = JavaUTF8(&[0xE2, 0x82, 0xAC]);
    assert_eq!(euro.len(), 3);
    assert_eq!(euro.to_string_checked().unwrap().chars().count(), 1);
    assert!(JavaUTF8(&[]).is_empty());
}

#[test]
fn display_renders_modified_utf8() {
    let s = JavaUTF8(&[b'h', 0xC3, 0xA9, b'y']);
    assert_eq!(s.to_string(), "héy");

    let with_nul = JavaUTF8(&[b'a', 0xC0, 0x80, b'b']);
    assert_eq!(with_nul.to_string(), "a\u{0}b");

    let emoji = JavaUTF8(&[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]);
    assert_eq!(emoji.to_string(), "\u{1F600}");
}

#[test]
fn display_of_malformed_bytes_does_not_panic() {
    let bad = JavaUTF8(&[0xF0, 0xFF, 0x00]);
    let shown = bad.to_string();
    assert!(shown.contains('\u{FFFD}'), "expected a replacement character, got {shown:?}");
}

#[test]
fn equality_is_by_bytes() {
    assert_eq!(JavaUTF8(b"Code"), JavaUTF8(b"Code"));
    assert_ne!(JavaUTF8(b"Code"), JavaUTF8(b"Cod"));
    assert_ne!(JavaUTF8(b"Code"), JavaUTF8(b"Code "));
}

#[test]
fn the_string_from_the_constants_fixture_round_trips() {
    // fixtures/Constants.java: "h\\u00e9llo \\u20ac \\uD83D\\uDE00 \\u0000 end"
    let bytes: &[u8] = &[
        b'h', 0xc3, 0xa9, b'l', b'l', b'o', b' ', 0xe2, 0x82, 0xac, b' ', 0xed, 0xa0, 0xbd, 0xed,
        0xb8, 0x80, b' ', 0xc0, 0x80, b' ', b'e', b'n', b'd',
    ];
    let s = JavaUTF8(bytes);
    assert!(s.is_valid());
    assert_eq!(s.to_string_checked().unwrap(), "héllo € \u{1F600} \u{0} end");
    // And it is not valid standard UTF-8, which is the whole point.
    assert!(std::str::from_utf8(bytes).is_err());
}
