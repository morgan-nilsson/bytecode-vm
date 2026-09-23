//! The constant pool (JVMS 4.4): every entry kind, the pool-wide rules that
//! govern it (two-slot Long/Double, cross references, version restrictions),
//! and the ways a malformed pool has to be rejected.
//!
//! Ported from `c_backup/tests/test_classfile_constant_pool.c`. The C suite
//! could read one entry at a time with `constant_pool_entry_read`; here every
//! case goes through `ClassFile::parse`, so each entry under test is placed in
//! the pool of a whole class and checked with `ClassFile::constant`.


use crate::common::*;

use bytecode_vm::parser::class_file::{ClassParserError, ConstantPoolEntry, MethodHandleKind};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// Matches one resolved constant pool entry, panicking with the index if the
/// slot is empty or holds a different kind.
macro_rules! entry {
    ($cf:expr, $index:expr, $pat:pat => $body:expr) => {{
        let index = $index;
        match $cf.constant(index) {
            Some($pat) => $body,
            Some(_) => panic!("constant #{index} is not a {}", stringify!($pat)),
            None => panic!("constant #{index} is missing or unusable"),
        }
    }};
}

/// `public class Test` at `major` with whatever `f` adds to its pool. Returns
/// the class bytes and whatever index `f` handed back.
fn class_with<T>(major: u16, f: impl FnOnce(&mut PoolBuilder) -> T) -> (Vec<u8>, T) {
    let mut cb = ClassBuilder::new(major);
    let value = f(&mut cb.pool);
    (cb.to_bytes(), value)
}

/// Like `class_with`, at the version most tests use.
fn class_with_default<T>(f: impl FnOnce(&mut PoolBuilder) -> T) -> (Vec<u8>, T) {
    class_with(DEFAULT_MAJOR_VERSION, f)
}

/// Bytes that end where the constant pool does, for entries whose payload is
/// deliberately incomplete: the header before the pool is always
/// magic(4) + minor(2) + major(2) + constant_pool_count(2).
fn cut_after_pool(cb: &ClassBuilder) -> Vec<u8> {
    const HEADER_LEN: usize = 10;
    let mut bytes = cb.to_bytes();
    bytes.truncate(HEADER_LEN + cb.pool.bytes.len());
    bytes
}

/// A class whose pool holds a CONSTANT_MethodHandle of `kind` over a member
/// reference of `ref_tag` named `name`.
fn method_handle_class(major: u16, kind: u8, ref_tag: u8, name: &str) -> Vec<u8> {
    let mut cb = ClassBuilder::new(major);
    // A field reference needs a field descriptor, a method reference a method one.
    let desc = if ref_tag == tag::FIELDREF { "I" } else { "()V" };
    let r = cb.pool.member_ref(ref_tag, "Test", name, desc);
    cb.pool.method_handle(kind, r);
    cb.to_bytes()
}

/// A class at `major` whose pool holds one entry of `entry_tag`, plus whatever
/// else that entry needs to be well formed.
fn class_with_tag_at_version(entry_tag: u8, major: u16) -> Vec<u8> {
    let mut cb = ClassBuilder::new(major);
    match entry_tag {
        tag::METHOD_HANDLE => {
            let m = cb.pool.methodref("Test", "m", "()V");
            cb.pool.method_handle(refkind::INVOKE_STATIC, m);
        }
        tag::METHOD_TYPE => {
            cb.pool.method_type("()V");
        }
        tag::INVOKE_DYNAMIC => {
            let nat = cb.pool.name_and_type("x", "()V");
            cb.pool.u2u2_entry(tag::INVOKE_DYNAMIC, 0, nat);
        }
        tag::DYNAMIC => {
            // A Dynamic names a value, so its descriptor is a field descriptor.
            let nat = cb.pool.name_and_type("x", "I");
            cb.pool.u2u2_entry(tag::DYNAMIC, 0, nat);
        }
        other => panic!("no builder for tag {other}"),
    }
    if entry_tag == tag::INVOKE_DYNAMIC || entry_tag == tag::DYNAMIC {
        // The bootstrap handle is itself a CONSTANT_MethodHandle, so below
        // version 51 such a class has two reasons to be rejected.
        let bsm = cb.pool.bootstrap_handle();
        cb.reserve_attributes(1);
        ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);
    }
    cb.to_bytes()
}

/// A minimal well-formed `module-info` (JVMS 4.1): ACC_MODULE alone, no super
/// class, no members, and the Module attribute such a class must carry.
/// `exports` names a package the module exports, which is how a
/// CONSTANT_Package legitimately reaches the pool.
fn module_info_class(major: u16, exports: Option<&str>) -> Vec<u8> {
    let mut cb = ClassBuilder::empty(major);
    cb.access_flags = acc::MODULE;
    cb.this_class = cb.pool.class("module-info");

    let module = cb.pool.module("com.example");
    // Every module but java.base implicitly requires it (JVMS 4.7.25).
    let java_base = cb.pool.module("java.base");
    let package = exports.map(|name| cb.pool.package(name));

    let attr_name = cb.pool.utf8("Module");
    cb.reserve_attributes(1);
    let body = cb.attributes.attr_begin(attr_name);
    cb.attributes.u2(module);
    cb.attributes.u2(0); // module_flags
    cb.attributes.u2(0); // module_version_index: absent
    cb.attributes.u2(1); // requires_count
    cb.attributes.u2(java_base);
    cb.attributes.u2(acc::MANDATED);
    cb.attributes.u2(0); // requires_version_index: absent
    match package {
        Some(p) => {
            cb.attributes.u2(1); // exports_count
            cb.attributes.u2(p);
            cb.attributes.u2(0); // exports_flags
            cb.attributes.u2(0); // exports_to_count: exported to everyone
        }
        None => {
            cb.attributes.u2(0); // exports_count
        }
    }
    cb.attributes.u2(0); // opens_count
    cb.attributes.u2(0); // uses_count
    cb.attributes.u2(0); // provides_count
    cb.attributes.attr_end(body);

    cb.to_bytes()
}

// ---------------------------------------------------------------------------
// CONSTANT_Utf8 (4.4.7)
// ---------------------------------------------------------------------------

#[test]
fn utf8_ascii() {
    let (bytes, i) = class_with_default(|p| p.utf8("hello"));
    assert_parses(&bytes, |cf| entry!(cf, i, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "hello")));
}

#[test]
fn utf8_empty_string() {
    let (bytes, i) = class_with_default(|p| p.utf8(""));
    assert_parses(&bytes, |cf| entry!(cf, i, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "")));
}

#[test]
fn utf8_length_is_exactly_the_declared_length() {
    // The entry that follows must not leak into the value.
    let (bytes, (first, second)) = class_with_default(|p| (p.utf8("abc"), p.utf8("xyz")));
    assert_parses(&bytes, |cf| {
        entry!(cf, first, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "abc"));
        entry!(cf, second, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "xyz"));
    });
}

#[test]
fn utf8_every_byte_from_one_to_seven_f() {
    // 0x01..0x7F all stand for themselves; only 0x00 is special in this range.
    let raw: Vec<u8> = (1u8..=0x7f).collect();
    let (bytes, i) = class_with_default(|p| p.utf8_bytes(&raw));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::UTF8(s) => assert_eq!(s.as_bytes(), &raw[..]))
    });
}

#[test]
fn utf8_two_byte_sequence() {
    // U+00E9 'é'
    let (bytes, i) = class_with_default(|p| p.utf8_bytes(&[0xC3, 0xA9]));
    assert_parses(&bytes, |cf| entry!(cf, i, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "é")));
}

#[test]
fn utf8_three_byte_sequence() {
    // U+20AC '€'
    let (bytes, i) = class_with_default(|p| p.utf8_bytes(&[0xE2, 0x82, 0xAC]));
    assert_parses(&bytes, |cf| entry!(cf, i, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "€")));
}

#[test]
fn utf8_modified_encoding_of_nul() {
    // Modified UTF-8 encodes U+0000 as C0 80 so a string never holds a zero byte.
    // The pool keeps the bytes as written, which is why they cannot simply be
    // compared against a Rust string: that would be one byte shorter.
    let (bytes, i) = class_with_default(|p| p.utf8_bytes(&[b'a', 0xC0, 0x80, b'b']));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::UTF8(s) => {
            assert_eq!(s.as_bytes(), &[b'a', 0xC0, 0x80, b'b']);
            assert_eq!(s.to_string_checked().unwrap(), "a\u{0}b");
        })
    });
}

#[test]
fn utf8_supplementary_character_as_surrogate_pair() {
    // U+1F600 is stored as two 3-byte surrogates rather than the 4-byte form
    // standard UTF-8 uses, so the stored bytes are longer than the Rust string.
    let (bytes, i) = class_with_default(|p| p.utf8_bytes(&[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::UTF8(s) => {
            assert_eq!(s.as_bytes(), &[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]);
            assert_eq!(s.to_string_checked().unwrap(), "\u{1F600}");
        })
    });
}

#[test]
fn utf8_maximum_length_65535() {
    let raw = vec![b'x'; 65535];
    let (bytes, i) = class_with_default(|p| p.utf8_bytes(&raw));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::UTF8(s) => assert_eq!(s.len(), 65535))
    });
}

#[test]
fn utf8_length_is_unsigned() {
    // 0x8000 must be read as 32768, not as a negative number.
    let raw = vec![b'y'; 0x8000];
    let (bytes, i) = class_with_default(|p| p.utf8_bytes(&raw));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::UTF8(s) => assert_eq!(s.len(), 0x8000))
    });
}

#[test]
fn utf8_raw_zero_byte_rejected() {
    // "No byte may have the value (byte)0."
    let (bytes, _) = class_with_default(|p| p.utf8_bytes(&[b'a', 0x00, b'b']));
    assert_rejected_with!(&bytes, ClassParserError::InvalidUTF8(_));
}

#[test]
fn utf8_bytes_f0_to_ff_rejected() {
    // "No byte may lie in the range (byte)0xf0 to (byte)0xff."
    for bad in 0xF0u8..=0xFF {
        let (bytes, _) = class_with_default(|p| p.utf8_bytes(&[b'a', bad, b'b']));
        assert_rejected_with!(&bytes, ClassParserError::InvalidUTF8(_));
    }
}

#[test]
fn utf8_four_byte_standard_utf8_rejected() {
    // Standard UTF-8 for U+1F600 starts with 0xF0, which modified UTF-8 forbids.
    let (bytes, _) = class_with_default(|p| p.utf8_bytes(&[0xF0, 0x9F, 0x98, 0x80]));
    assert_rejected_with!(&bytes, ClassParserError::InvalidUTF8(_));
}

#[test]
fn utf8_lone_continuation_byte_rejected() {
    let (bytes, _) = class_with_default(|p| p.utf8_bytes(&[b'a', 0x80, b'b']));
    assert_rejected_with!(&bytes, ClassParserError::InvalidUTF8(_));
}

#[test]
fn utf8_truncated_multibyte_sequence_rejected() {
    // The 3-byte sequence starting E2 82 needs one more continuation byte. The
    // entry's declared length still covers every byte present, so the file is
    // intact and only the encoding is at fault.
    let (bytes, _) = class_with_default(|p| p.utf8_bytes(&[b'a', 0xE2, 0x82]));
    assert_rejected_with!(&bytes, ClassParserError::InvalidUTF8(_));
}

#[test]
fn utf8_unpaired_surrogate_accepted() {
    // A CONSTANT_Utf8 holds UTF-16 code units, so an unpaired surrogate is a
    // legal constant: javac emits ED A0 BD for the string "a\uD83Db", and
    // HotSpot loads it. Only the byte-level rules of JVMS 4.4.7 are enforced.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let i = cb.pool.utf8_bytes(&[b'a', 0xED, 0xA0, 0xBD, b'b']);
    let bytes = cb.to_bytes();
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::UTF8(s) => {
            assert_eq!(s.as_bytes(), &[b'a', 0xED, 0xA0, 0xBD, b'b']);
        })
    });
}

#[test]
fn utf8_length_past_end_of_file_rejected() {
    // A declared length far larger than the rest of the file.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool.bytes.u1(tag::UTF8);
    cb.pool.bytes.u2(0xFFFF);
    cb.pool.bytes.raw(b"abc");
    cb.pool.next += 1;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::Truncated(_));
}

#[test]
fn utf8_missing_length_rejected() {
    // Only one of the two length bytes is present before the file ends.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool.bytes.u1(tag::UTF8);
    cb.pool.bytes.u1(0);
    cb.pool.next += 1;
    assert_rejected_with!(&cut_after_pool(&cb), ClassParserError::Truncated(_));
}

// ---------------------------------------------------------------------------
// CONSTANT_Integer / CONSTANT_Float (4.4.4)
// ---------------------------------------------------------------------------

#[test]
fn integer_values() {
    for v in [0, 1, -1, 0x1234_5678, i32::MAX, i32::MIN] {
        let (bytes, i) = class_with_default(|p| p.integer(v));
        assert_parses(&bytes, |cf| {
            entry!(cf, i, ConstantPoolEntry::Integer(n) => assert_eq!(*n, v))
        });
    }
}

#[test]
fn integer_byte_order() {
    // Distinct bytes, so a byte-swapped read cannot pass.
    let (bytes, i) = class_with_default(|p| p.integer(0x0102_0304));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::Integer(n) => assert_eq!(*n, 0x0102_0304))
    });
}

#[test]
fn float_pi() {
    let (bytes, i) = class_with_default(|p| p.float_bits(0x4049_0FDB));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::Float(f) => {
            assert!(float_bits_eq(*f, 0x4049_0FDB));
            assert!(*f > 3.14159_f32 && *f < 3.14160_f32);
        })
    });
}

#[test]
fn float_one_and_negative() {
    let (bytes, i) = class_with_default(|p| p.float_bits(0x3F80_0000));
    assert_parses(&bytes, |cf| entry!(cf, i, ConstantPoolEntry::Float(f) => assert_eq!(*f, 1.0)));

    let (bytes, i) = class_with_default(|p| p.float_bits(0xC000_0000));
    assert_parses(&bytes, |cf| entry!(cf, i, ConstantPoolEntry::Float(f) => assert_eq!(*f, -2.0)));
}

#[test]
fn float_zero_and_negative_zero() {
    // 0.0 == -0.0, so only the sign bit tells the two apart.
    for bits in [0x0000_0000u32, 0x8000_0000] {
        let (bytes, i) = class_with_default(|p| p.float_bits(bits));
        assert_parses(&bytes, |cf| {
            entry!(cf, i, ConstantPoolEntry::Float(f) => {
                assert_eq!(*f, 0.0);
                assert!(float_bits_eq(*f, bits));
            })
        });
    }
}

#[test]
fn float_infinities() {
    for (bits, positive) in [(0x7F80_0000u32, true), (0xFF80_0000, false)] {
        let (bytes, i) = class_with_default(|p| p.float_bits(bits));
        assert_parses(&bytes, |cf| {
            entry!(cf, i, ConstantPoolEntry::Float(f) => {
                assert!(f.is_infinite());
                assert_eq!(f.is_sign_positive(), positive);
            })
        });
    }
}

#[test]
fn float_nan() {
    // Any bits in 0x7f800001..0x7fffffff or 0xff800001..0xffffffff are NaN, and
    // the payload must survive the round trip rather than be canonicalised.
    for bits in [0x7FC0_0000u32, 0xFFC0_0001] {
        let (bytes, i) = class_with_default(|p| p.float_bits(bits));
        assert_parses(&bytes, |cf| {
            entry!(cf, i, ConstantPoolEntry::Float(f) => {
                assert!(f.is_nan());
                assert!(float_bits_eq(*f, bits));
            })
        });
    }
}

#[test]
fn float_smallest_denormal_and_max() {
    for bits in [0x0000_0001u32, 0x7F7F_FFFF] {
        let (bytes, i) = class_with_default(|p| p.float_bits(bits));
        assert_parses(&bytes, |cf| {
            entry!(cf, i, ConstantPoolEntry::Float(f) => assert!(float_bits_eq(*f, bits)))
        });
    }
}

// ---------------------------------------------------------------------------
// CONSTANT_Long / CONSTANT_Double (4.4.5)
// ---------------------------------------------------------------------------

#[test]
fn long_high_bytes_come_first() {
    // high_bytes then low_bytes, with every byte distinct so a swapped or
    // reordered pair of 32-bit reads cannot pass.
    let (bytes, i) = class_with_default(|p| p.long(0x0123_4567_89AB_CDEF));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::Long(v) => assert_eq!(*v, 0x0123_4567_89AB_CDEF))
    });
}

#[test]
fn long_values() {
    for v in [0, 1, -1, 0x0000_0000_FFFF_FFFF, 0x0000_0001_0000_0000, i64::MAX, i64::MIN] {
        let (bytes, i) = class_with_default(|p| p.long(v));
        assert_parses(&bytes, |cf| entry!(cf, i, ConstantPoolEntry::Long(n) => assert_eq!(*n, v)));
    }
}

#[test]
fn double_pi() {
    let (bytes, i) = class_with_default(|p| p.double_bits(0x4009_21FB_5444_2D18));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::Double(d) => {
            assert!(double_bits_eq(*d, 0x4009_21FB_5444_2D18));
            assert_eq!(*d, std::f64::consts::PI);
        })
    });
}

#[test]
fn double_high_bytes_come_first() {
    // Distinct high and low halves, so a swapped read cannot pass.
    let (bytes, i) = class_with_default(|p| p.double_bits(0x3FF0_0000_0000_0001));
    assert_parses(&bytes, |cf| {
        entry!(cf, i, ConstantPoolEntry::Double(d) => {
            assert!(double_bits_eq(*d, 0x3FF0_0000_0000_0001))
        })
    });
}

#[test]
fn double_special_values() {
    for bits in [
        0x8000_0000_0000_0000u64, // -0.0
        0x7FF0_0000_0000_0000,    // +inf
        0xFFF0_0000_0000_0000,    // -inf
        0x7FF8_0000_0000_0000,    // NaN
        0x0000_0000_0000_0001,    // smallest denormal
    ] {
        let (bytes, i) = class_with_default(|p| p.double_bits(bits));
        assert_parses(&bytes, |cf| {
            entry!(cf, i, ConstantPoolEntry::Double(d) => assert!(double_bits_eq(*d, bits)))
        });
    }
}

// ---------------------------------------------------------------------------
// Index-only and reference entries (4.4.1 - 4.4.3, 4.4.6, 4.4.8 - 4.4.12)
// ---------------------------------------------------------------------------
//
// The C suite read these in isolation with arbitrary indices; here the indices
// have to point at pool entries of the right kind, so each test names a real
// target and checks the entry points back at it.

#[test]
fn class_entry() {
    let (bytes, (name, class)) = class_with_default(|p| {
        let name = p.utf8("java/lang/String");
        (name, p.class_at(name))
    });
    assert_parses(&bytes, |cf| {
        entry!(cf, class, ConstantPoolEntry::ClassIndex(n) => assert_eq!(*n, name));
        entry!(cf, name, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "java/lang/String"));
    });
}

#[test]
fn class_name_may_be_an_array_type() {
    // JVMS 4.4.1: a Class entry naming an array type holds a descriptor, not a
    // binary class name.
    let (bytes, class) = class_with_default(|p| p.class("[Ljava/lang/String;"));
    assert_parses(&bytes, |cf| {
        entry!(cf, class, ConstantPoolEntry::ClassIndex(_) => ());
    });
}

#[test]
fn class_name_as_object_descriptor_rejected() {
    // A non-array Class entry holds a binary name; "Ljava/lang/String;" is a
    // field descriptor and is not one.
    let (bytes, _) = class_with_default(|p| p.class("Ljava/lang/String;"));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn string_entry() {
    let (bytes, (text, string)) = class_with_default(|p| {
        let text = p.utf8("a string");
        (text, p.u2_entry(tag::STRING, text))
    });
    assert_parses(&bytes, |cf| {
        entry!(cf, string, ConstantPoolEntry::StringIndex(n) => assert_eq!(*n, text));
    });
}

#[test]
fn fieldref_entry() {
    let (bytes, r) = class_with_default(|p| p.fieldref("Test", "field", "I"));
    assert_parses(&bytes, |cf| {
        entry!(cf, r, ConstantPoolEntry::FieldRef { class_index, name_and_type_index } => {
            entry!(cf, *class_index, ConstantPoolEntry::ClassIndex(_) => ());
            entry!(cf, *name_and_type_index, ConstantPoolEntry::NameAndType { .. } => ());
        })
    });
}

#[test]
fn methodref_entry() {
    let (bytes, r) = class_with_default(|p| p.methodref("Test", "method", "()V"));
    assert_parses(&bytes, |cf| {
        entry!(cf, r, ConstantPoolEntry::MethodRef { class_index, name_and_type_index } => {
            entry!(cf, *class_index, ConstantPoolEntry::ClassIndex(_) => ());
            entry!(cf, *name_and_type_index, ConstantPoolEntry::NameAndType { .. } => ());
        })
    });
}

#[test]
fn interface_methodref_entry() {
    let (bytes, r) = class_with_default(|p| p.interface_methodref("java/lang/Runnable", "run", "()V"));
    assert_parses(&bytes, |cf| {
        entry!(cf, r, ConstantPoolEntry::InterfaceMethodRef { class_index, .. } => {
            entry!(cf, *class_index, ConstantPoolEntry::ClassIndex(_) => ());
        })
    });
}

#[test]
fn name_and_type_entry() {
    let (bytes, nat) = class_with_default(|p| p.name_and_type("value", "J"));
    assert_parses(&bytes, |cf| {
        entry!(cf, nat, ConstantPoolEntry::NameAndType { name_index, descriptor_index } => {
            entry!(cf, *name_index, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "value"));
            entry!(cf, *descriptor_index, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "J"));
        })
    });
}

#[test]
fn method_handle_entry_every_kind() {
    // Every reference kind, each over the member reference kind it requires.
    // The byte and the variant are both written out, so a wrong mapping in the
    // parser cannot agree with a wrong mapping here.
    for (kind, expected, ref_tag, name) in [
        (refkind::GET_FIELD, MethodHandleKind::GetField, tag::FIELDREF, "f"),
        (refkind::GET_STATIC, MethodHandleKind::GetStatic, tag::FIELDREF, "f"),
        (refkind::PUT_FIELD, MethodHandleKind::PutField, tag::FIELDREF, "f"),
        (refkind::PUT_STATIC, MethodHandleKind::PutStatic, tag::FIELDREF, "f"),
        (refkind::INVOKE_VIRTUAL, MethodHandleKind::InvokeVirtual, tag::METHODREF, "m"),
        (refkind::INVOKE_STATIC, MethodHandleKind::InvokeStatic, tag::METHODREF, "m"),
        (refkind::INVOKE_SPECIAL, MethodHandleKind::InvokeSpecial, tag::METHODREF, "m"),
        (refkind::NEW_INVOKE_SPECIAL, MethodHandleKind::NewInvokeSpecial, tag::METHODREF, "<init>"),
        (refkind::INVOKE_INTERFACE, MethodHandleKind::InvokeInterface, tag::INTERFACE_METHODREF, "m"),
    ] {
        let bytes = method_handle_class(DEFAULT_MAJOR_VERSION, kind, ref_tag, name);
        assert_parses(&bytes, |cf| {
            // The handle is the pool's last entry.
            let last = cf.constant_pool.len() as u16;
            entry!(cf, last, ConstantPoolEntry::MethodHandle { ref_kind, .. } => {
                assert_eq!(*ref_kind, expected)
            })
        });
    }
}

#[test]
fn method_handle_kind_is_one_byte() {
    // reference_kind is u1; a reader taking u2 would swallow half the index.
    let (bytes, (r, mh)) = class_with_default(|p| {
        let r = p.methodref("Test", "m", "()V");
        (r, p.method_handle(refkind::INVOKE_STATIC, r))
    });
    assert_parses(&bytes, |cf| {
        entry!(cf, mh, ConstantPoolEntry::MethodHandle { ref_kind, ref_index } => {
            assert_eq!(*ref_kind, MethodHandleKind::InvokeStatic);
            assert_eq!(*ref_index, r);
        })
    });
}

#[test]
fn method_type_entry() {
    let (bytes, mt) = class_with_default(|p| p.method_type("(IJ)Ljava/lang/String;"));
    assert_parses(&bytes, |cf| {
        entry!(cf, mt, ConstantPoolEntry::MethodTypeIndex(n) => {
            entry!(cf, *n, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "(IJ)Ljava/lang/String;"));
        })
    });
}

#[test]
fn dynamic_entry() {
    let mut cb = ClassBuilder::new(55);
    let nat = cb.pool.name_and_type("dyn", "I");
    let dy = cb.pool.u2u2_entry(tag::DYNAMIC, 0, nat);
    let bsm = cb.pool.bootstrap_handle();
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);

    assert_parses(&cb.to_bytes(), |cf| {
        entry!(cf, dy, ConstantPoolEntry::Dynamic { bootstrap_index, name_and_type_index } => {
            assert_eq!(*bootstrap_index, 0);
            assert_eq!(*name_and_type_index, nat);
        })
    });
}

#[test]
fn invoke_dynamic_entry() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let nat = cb.pool.name_and_type("call", "()V");
    let indy = cb.pool.u2u2_entry(tag::INVOKE_DYNAMIC, 0, nat);
    let bsm = cb.pool.bootstrap_handle();
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);

    assert_parses(&cb.to_bytes(), |cf| {
        entry!(cf, indy, ConstantPoolEntry::InvokeDynamic { bootstrap_index, name_and_type_index } => {
            assert_eq!(*bootstrap_index, 0);
            assert_eq!(*name_and_type_index, nat);
        })
    });
}

#[test]
fn module_entry() {
    // CONSTANT_Module only exists in a module-info class, so the entry under
    // test is the one the Module attribute names.
    let bytes = module_info_class(53, None);
    assert_parses(&bytes, |cf| {
        assert!(
            cf.constant_pool
                .iter()
                .any(|e| matches!(e, ConstantPoolEntry::ModuleIndex(_))),
            "no CONSTANT_Module entry survived parsing"
        );
    });
}

#[test]
fn package_entry() {
    let bytes = module_info_class(53, Some("com/example/api"));
    assert_parses(&bytes, |cf| {
        assert!(
            cf.constant_pool
                .iter()
                .any(|e| matches!(e, ConstantPoolEntry::PackageIndex(_))),
            "no CONSTANT_Package entry survived parsing"
        );
    });
}

// ---------------------------------------------------------------------------
// Bad tags and truncated entries
// ---------------------------------------------------------------------------

#[test]
fn invalid_tags_rejected() {
    // 2 was never assigned, 13/14 and 21+ are gaps in table 4.4-A, and the high
    // values check that the tag is read as an unsigned byte.
    for bad in [0u8, 2, 13, 14, 21, 22, 100, 127, 128, 254, 255] {
        let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
        cb.pool.bytes.u1(bad);
        cb.pool.bytes.u4(0); // some payload, so EOF isn't the reason it fails
        cb.pool.next += 1;
        assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolTag);
    }
}

#[test]
fn entry_at_eof_rejected() {
    // The count promises one more entry than the file actually holds.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool_count_override = Some(cb.pool.next + 1);
    assert_rejected_with!(&cut_after_pool(&cb), ClassParserError::Truncated(_));
}

#[test]
fn truncated_payload_rejected_for_every_tag() {
    let kinds: &[(u8, usize)] = &[
        (tag::INTEGER, 4),
        (tag::FLOAT, 4),
        (tag::LONG, 8),
        (tag::DOUBLE, 8),
        (tag::CLASS, 2),
        (tag::STRING, 2),
        (tag::FIELDREF, 4),
        (tag::METHODREF, 4),
        (tag::INTERFACE_METHODREF, 4),
        (tag::NAME_AND_TYPE, 4),
        (tag::METHOD_HANDLE, 3),
        (tag::METHOD_TYPE, 2),
        (tag::DYNAMIC, 4),
        (tag::INVOKE_DYNAMIC, 4),
        (tag::MODULE, 2),
        (tag::PACKAGE, 2),
    ];
    for &(entry_tag, payload) in kinds {
        for have in 0..payload {
            let mut cb = ClassBuilder::new(MAX_MAJOR_VERSION);
            cb.pool.bytes.u1(entry_tag);
            cb.pool.bytes.fill(0x01, have);
            cb.pool.next += 1;
            let bytes = cut_after_pool(&cb);
            assert_rejected_with!(&bytes, ClassParserError::Truncated(_));
        }
    }
}

#[test]
fn consecutive_entries_read_from_one_stream() {
    // Each entry has a different payload width, so a miscounted one would throw
    // every later index off.
    let (bytes, (u, i, l, after)) = class_with_default(|p| {
        (p.utf8("abc"), p.integer(99), p.long(-5), p.utf8("after"))
    });
    assert_parses(&bytes, |cf| {
        entry!(cf, u, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "abc"));
        entry!(cf, i, ConstantPoolEntry::Integer(n) => assert_eq!(*n, 99));
        entry!(cf, l, ConstantPoolEntry::Long(n) => assert_eq!(*n, -5));
        entry!(cf, after, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "after"));
    });
}

#[test]
fn constant_pool_count_zero_rejected() {
    // constant_pool_count is the number of entries plus one, so it is never 0.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool_count_override = Some(0);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPool);
}

// ---------------------------------------------------------------------------
// Pool-wide rules
// ---------------------------------------------------------------------------

#[test]
fn long_occupies_two_slots() {
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    let l = cb.pool.long(0x1122_3344_5566_7788);
    assert_eq!(l, 1);
    cb.access_flags = acc::PUBLIC | acc::SUPER;
    cb.this_class = cb.pool.class("Test"); // Utf8 at #3, Class at #4
    cb.super_class = cb.pool.class("java/lang/Object");
    assert_eq!(cb.this_class, 4);

    assert_parses(&cb.to_bytes(), |cf| {
        // constant_pool_count is 7, so the pool holds entries #1..=#6.
        assert_eq!(cf.constant_pool.len(), 6);
        entry!(cf, l, ConstantPoolEntry::Long(v) => assert_eq!(*v, 0x1122_3344_5566_7788));
        // #2 is the unusable second half of the Long.
        assert!(cf.constant(2).is_none());
        entry!(cf, 3, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "Test"));
        entry!(cf, 4, ConstantPoolEntry::ClassIndex(n) => assert_eq!(*n, 3));
        assert_eq!(cf.this_class.as_bytes(), b"Test");
    });
}

#[test]
fn double_occupies_two_slots() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let d = cb.pool.double_bits(0x4009_21FB_5444_2D18);
    let after = cb.pool.utf8("after");
    assert_eq!(after, d + 2);

    assert_parses(&cb.to_bytes(), |cf| {
        assert_eq!(cf.constant_pool.len(), usize::from(after));
        entry!(cf, d, ConstantPoolEntry::Double(v) => {
            assert!(double_bits_eq(*v, 0x4009_21FB_5444_2D18))
        });
        assert!(cf.constant(d + 1).is_none());
        entry!(cf, after, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "after"));
    });
}

#[test]
fn many_consecutive_longs_and_doubles() {
    // Forty entries in a row, each skipping a slot: an off-by-one anywhere in
    // the run moves every index after it.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let mut idx = [0u16; 40];
    for (i, slot) in idx.iter_mut().enumerate() {
        *slot = if i % 2 == 1 {
            cb.pool.long(i as i64 * 1_000_000_007)
        } else {
            cb.pool.double_bits(0x4000_0000_0000_0000 + i as u64)
        };
    }
    let tail = cb.pool.utf8("tail");

    assert_parses(&cb.to_bytes(), |cf| {
        for (i, &slot) in idx.iter().enumerate() {
            if i % 2 == 1 {
                entry!(cf, slot, ConstantPoolEntry::Long(v) => {
                    assert_eq!(*v, i as i64 * 1_000_000_007)
                });
            } else {
                entry!(cf, slot, ConstantPoolEntry::Double(v) => {
                    assert!(double_bits_eq(*v, 0x4000_0000_0000_0000 + i as u64))
                });
            }
            assert!(cf.constant(slot + 1).is_none());
        }
        entry!(cf, tail, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "tail"));
    });
}

#[test]
fn long_as_last_entry_needs_room_for_second_slot() {
    // A Long at index count-1 would put its unusable slot past the end. Which
    // error names that is not settled: the count is only impossible in the light
    // of the entries, and no entry holds a bad index.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool.long(1);
    cb.pool_count_override = Some(cb.pool.next - 1);
    assert_rejected(&cb.to_bytes());
}

#[test]
fn long_as_last_entry_with_count_including_second_slot() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let l = cb.pool.long(77);
    assert_parses(&cb.to_bytes(), |cf| {
        assert_eq!(cf.constant_pool.len(), usize::from(l) + 1);
        entry!(cf, l, ConstantPoolEntry::Long(v) => assert_eq!(*v, 77));
        assert!(cf.constant(l + 1).is_none());
    });
}

#[test]
fn reference_to_unusable_long_slot_rejected() {
    // The slot after a Long is not a valid target for any index.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let l = cb.pool.long(1);
    cb.add_interface(l + 1);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn forward_references_within_pool_accepted() {
    // Entries may refer to entries that appear later in the pool, so resolution
    // cannot happen while entries are still being read.
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::SUPER;
    cb.this_class = cb.pool.class_at(3); // #1 -> #3
    cb.super_class = cb.pool.class_at(4); // #2 -> #4
    cb.pool.utf8("Test"); // #3
    cb.pool.utf8("java/lang/Object"); // #4

    assert_parses(&cb.to_bytes(), |cf| {
        assert_eq!(cf.this_class.as_bytes(), b"Test");
        assert_eq!(cf.super_class.as_ref().map(|s| s.as_bytes()), Some(&b"java/lang/Object"[..]));
    });
}

#[test]
fn duplicate_entries_accepted() {
    // JVMS does not require the pool to be deduplicated; two Utf8 entries with
    // identical contents are independent constants.
    let (bytes, (a, b)) = class_with_default(|p| (p.utf8("same"), p.utf8("same")));
    assert_parses(&bytes, |cf| {
        assert_ne!(a, b);
        entry!(cf, a, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "same"));
        entry!(cf, b, ConstantPoolEntry::UTF8(s) => assert_eq!(s, "same"));
    });
}

#[test]
fn every_entry_kind_in_one_class() {
    let mut cb = ClassBuilder::new(55);
    let i = cb.pool.integer(-42);
    let f = cb.pool.float_bits(0x3F80_0000);
    let l = cb.pool.long(i64::MIN);
    let d = cb.pool.double_bits(0xBFF0_0000_0000_0000);
    let s = cb.pool.string("str");
    let fr = cb.pool.fieldref("Test", "field", "I");
    let mr = cb.pool.methodref("Test", "method", "()V");
    let imr = cb.pool.interface_methodref("java/lang/Runnable", "run", "()V");
    let mh = cb.pool.method_handle(refkind::INVOKE_STATIC, mr);
    let mt = cb.pool.method_type("(I)J");
    let nat = cb.pool.name_and_type("dyn", "I");
    let bsm = cb.pool.bootstrap_handle();
    let dy = cb.pool.u2u2_entry(tag::DYNAMIC, 0, nat);
    let indy_nat = cb.pool.name_and_type("call", "()V");
    let indy = cb.pool.u2u2_entry(tag::INVOKE_DYNAMIC, 0, indy_nat);
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);

    assert_parses(&cb.to_bytes(), |cf| {
        entry!(cf, i, ConstantPoolEntry::Integer(v) => assert_eq!(*v, -42));
        entry!(cf, f, ConstantPoolEntry::Float(v) => assert_eq!(*v, 1.0));
        entry!(cf, l, ConstantPoolEntry::Long(v) => assert_eq!(*v, i64::MIN));
        entry!(cf, d, ConstantPoolEntry::Double(v) => assert_eq!(*v, -1.0));
        entry!(cf, s, ConstantPoolEntry::StringIndex(n) => {
            entry!(cf, *n, ConstantPoolEntry::UTF8(t) => assert_eq!(t, "str"));
        });
        entry!(cf, fr, ConstantPoolEntry::FieldRef { class_index, .. } => {
            entry!(cf, *class_index, ConstantPoolEntry::ClassIndex(n) => {
                entry!(cf, *n, ConstantPoolEntry::UTF8(t) => assert_eq!(t, "Test"));
            });
        });
        entry!(cf, mr, ConstantPoolEntry::MethodRef { .. } => ());
        entry!(cf, imr, ConstantPoolEntry::InterfaceMethodRef { class_index, .. } => {
            entry!(cf, *class_index, ConstantPoolEntry::ClassIndex(n) => {
                entry!(cf, *n, ConstantPoolEntry::UTF8(t) => assert_eq!(t, "java/lang/Runnable"));
            });
        });
        entry!(cf, mh, ConstantPoolEntry::MethodHandle { ref_index, .. } => {
            assert_eq!(*ref_index, mr)
        });
        entry!(cf, mt, ConstantPoolEntry::MethodTypeIndex(n) => {
            entry!(cf, *n, ConstantPoolEntry::UTF8(t) => assert_eq!(t, "(I)J"));
        });
        entry!(cf, dy, ConstantPoolEntry::Dynamic { name_and_type_index, .. } => {
            assert_eq!(*name_and_type_index, nat)
        });
        entry!(cf, indy, ConstantPoolEntry::InvokeDynamic { name_and_type_index, .. } => {
            assert_eq!(*name_and_type_index, indy_nat)
        });
    });
}

// ---------------------------------------------------------------------------
// Cross references (format checking)
// ---------------------------------------------------------------------------
//
// Two different failures live here and must not be confused. An index that
// cannot name an entry at all — 0, past the end, or the unusable slot after a
// Long or Double — is `ClassParseInvalidConstantPoolIndex`. An index that does
// name a real entry, but one of the wrong kind, is
// `ClassParseReferenceToInvalidConstantPoolEntry`. Each reference-bearing entry
// kind below is exercised both ways.

#[test]
fn class_name_index_not_utf8_rejected() {
    let (bytes, _) = class_with_default(|p| {
        let n = p.integer(3);
        p.class_at(n)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn class_name_index_zero_rejected() {
    // Index 0 is never a constant; only a few named fields may use it to mean
    // "absent", and name_index is not one of them.
    let (bytes, _) = class_with_default(|p| p.class_at(0));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn class_name_index_out_of_range_rejected() {
    let (bytes, _) = class_with_default(|p| p.class_at(500));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn class_name_index_pointing_at_unusable_slot_rejected() {
    let (bytes, _) = class_with_default(|p| {
        let l = p.long(1);
        p.class_at(l + 1)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn string_index_not_utf8_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool.u2_entry(tag::STRING, cb.this_class);
    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn string_index_zero_rejected() {
    let (bytes, _) = class_with_default(|p| p.u2_entry(tag::STRING, 0));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn string_index_pointing_at_unusable_slot_rejected() {
    // The slot after a Double holds no entry, so it is an unusable index rather
    // than a reference to something of the wrong kind.
    let (bytes, _) = class_with_default(|p| {
        let d = p.double_bits(0);
        p.u2_entry(tag::STRING, d + 1)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn fieldref_class_index_not_class_rejected() {
    let (bytes, _) = class_with_default(|p| {
        let nat = p.name_and_type("f", "I");
        let not_a_class = p.utf8("Test");
        p.u2u2_entry(tag::FIELDREF, not_a_class, nat)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn fieldref_class_index_zero_rejected() {
    let (bytes, _) = class_with_default(|p| {
        let nat = p.name_and_type("f", "I");
        p.u2u2_entry(tag::FIELDREF, 0, nat)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn methodref_name_and_type_index_not_name_and_type_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool.u2u2_entry(tag::METHODREF, cb.this_class, cb.super_class);
    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn methodref_name_and_type_index_out_of_range_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool.u2u2_entry(tag::METHODREF, cb.this_class, 999);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn interface_methodref_class_index_out_of_range_rejected() {
    let (bytes, _) = class_with_default(|p| {
        let nat = p.name_and_type("run", "()V");
        p.u2u2_entry(tag::INTERFACE_METHODREF, 999, nat)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn interface_methodref_class_index_not_class_rejected() {
    let (bytes, _) = class_with_default(|p| {
        let nat = p.name_and_type("run", "()V");
        let not_a_class = p.utf8("java/lang/Runnable");
        p.u2u2_entry(tag::INTERFACE_METHODREF, not_a_class, nat)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn name_and_type_indices_not_utf8_rejected() {
    let (bytes, _) = class_with_default(|p| {
        let name = p.integer(1);
        let desc = p.utf8("I");
        p.u2u2_entry(tag::NAME_AND_TYPE, name, desc)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn name_and_type_zero_indices_rejected() {
    for (name, desc) in [(0u16, 1u16), (1, 0)] {
        let (bytes, _) = class_with_default(|p| {
            p.utf8("I");
            p.u2u2_entry(tag::NAME_AND_TYPE, name, desc)
        });
        assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
    }
}

#[test]
fn name_and_type_with_malformed_descriptor_rejected() {
    // "Q" is neither a field descriptor nor a method descriptor.
    let (bytes, _) = class_with_default(|p| p.name_and_type("x", "Q"));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn fieldref_with_method_descriptor_rejected() {
    let (bytes, _) = class_with_default(|p| p.fieldref("Test", "f", "()V"));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn methodref_with_field_descriptor_rejected() {
    let (bytes, _) = class_with_default(|p| p.methodref("Test", "m", "I"));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn interface_methodref_with_field_descriptor_rejected() {
    let (bytes, _) = class_with_default(|p| p.interface_methodref("java/lang/Runnable", "m", "I"));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn method_type_index_not_utf8_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool.u2_entry(tag::METHOD_TYPE, cb.this_class);
    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn method_type_index_zero_rejected() {
    let (bytes, _) = class_with_default(|p| p.u2_entry(tag::METHOD_TYPE, 0));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn method_type_with_field_descriptor_rejected() {
    // A MethodType's descriptor is always a method descriptor (JVMS 4.4.9).
    let (bytes, _) = class_with_default(|p| p.method_type("I"));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

// ---------------------------------------------------------------------------
// CONSTANT_MethodHandle rules (4.4.8)
// ---------------------------------------------------------------------------

#[test]
fn method_handle_field_kinds_accept_fieldref() {
    for kind in refkind::GET_FIELD..=refkind::PUT_STATIC {
        let bytes = method_handle_class(DEFAULT_MAJOR_VERSION, kind, tag::FIELDREF, "f");
        assert_parses(&bytes, |_| ());
    }
}

#[test]
fn method_handle_field_kind_with_methodref_rejected() {
    let bytes = method_handle_class(DEFAULT_MAJOR_VERSION, refkind::GET_FIELD, tag::METHODREF, "m");
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn method_handle_invoke_virtual_with_fieldref_rejected() {
    let bytes =
        method_handle_class(DEFAULT_MAJOR_VERSION, refkind::INVOKE_VIRTUAL, tag::FIELDREF, "f");
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn method_handle_invoke_virtual_methodref_accepted() {
    let bytes =
        method_handle_class(DEFAULT_MAJOR_VERSION, refkind::INVOKE_VIRTUAL, tag::METHODREF, "m");
    assert_parses(&bytes, |_| ());
}

#[test]
fn method_handle_invoke_static_interface_methodref_needs_version_52() {
    // invokestatic and invokespecial could only name an InterfaceMethodref from
    // version 52 on (JVMS 4.4.8). The CONSTANT_MethodHandle tag itself is legal
    // at 51, so what fails is the kind/reference pairing, not the tag's version.
    for kind in [refkind::INVOKE_STATIC, refkind::INVOKE_SPECIAL] {
        assert_parses(&method_handle_class(52, kind, tag::INTERFACE_METHODREF, "m"), |_| ());
        assert_rejected_with!(
            &method_handle_class(51, kind, tag::INTERFACE_METHODREF, "m"),
            ClassParserError::ClassParseInvalidFeatureUsedForVersion
        );
    }
}

#[test]
fn method_handle_invoke_interface_requires_interface_methodref() {
    let bytes = method_handle_class(
        DEFAULT_MAJOR_VERSION,
        refkind::INVOKE_INTERFACE,
        tag::INTERFACE_METHODREF,
        "m",
    );
    assert_parses(&bytes, |_| ());

    let bytes =
        method_handle_class(DEFAULT_MAJOR_VERSION, refkind::INVOKE_INTERFACE, tag::METHODREF, "m");
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn method_handle_new_invoke_special_must_name_init() {
    let bytes = method_handle_class(
        DEFAULT_MAJOR_VERSION,
        refkind::NEW_INVOKE_SPECIAL,
        tag::METHODREF,
        "<init>",
    );
    assert_parses(&bytes, |_| ());

    let bytes =
        method_handle_class(DEFAULT_MAJOR_VERSION, refkind::NEW_INVOKE_SPECIAL, tag::METHODREF, "m");
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn method_handle_invoke_kinds_must_not_name_init_or_clinit() {
    let bytes = method_handle_class(
        DEFAULT_MAJOR_VERSION,
        refkind::INVOKE_VIRTUAL,
        tag::METHODREF,
        "<init>",
    );
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);

    let bytes = method_handle_class(
        DEFAULT_MAJOR_VERSION,
        refkind::INVOKE_STATIC,
        tag::METHODREF,
        "<clinit>",
    );
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn method_handle_kind_zero_rejected() {
    // reference_kind is its own field, not a pool tag, so an impossible one is
    // an ordinary format check.
    let bytes = method_handle_class(DEFAULT_MAJOR_VERSION, 0, tag::METHODREF, "m");
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn method_handle_kind_ten_rejected() {
    // Reference kinds run 1..=9; 10 is one past the end.
    let bytes = method_handle_class(DEFAULT_MAJOR_VERSION, 10, tag::METHODREF, "m");
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn method_handle_reference_index_zero_rejected() {
    let (bytes, _) = class_with_default(|p| p.method_handle(refkind::INVOKE_STATIC, 0));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn method_handle_reference_index_pointing_at_unusable_slot_rejected() {
    let (bytes, _) = class_with_default(|p| {
        let l = p.long(1);
        p.method_handle(refkind::INVOKE_STATIC, l + 1)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn method_handle_reference_index_not_a_member_ref_rejected() {
    // A Utf8 is a real entry, so this is the wrong kind of target rather than an
    // unusable index — and it is not the kind/reference mismatch of 4.4.8
    // either, which is about pairing two legal member reference kinds.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let not_a_ref = cb.pool.utf8("m");
    cb.pool.method_handle(refkind::INVOKE_STATIC, not_a_ref);
    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

// ---------------------------------------------------------------------------
// invokedynamic / dynamic need BootstrapMethods (4.4.10, 4.4.11, 4.7.23)
// ---------------------------------------------------------------------------

#[test]
fn invoke_dynamic_without_bootstrap_methods_rejected() {
    let (bytes, _) = class_with_default(|p| {
        let nat = p.name_and_type("x", "()V");
        p.u2u2_entry(tag::INVOKE_DYNAMIC, 0, nat)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn dynamic_without_bootstrap_methods_rejected() {
    let (bytes, _) = class_with(55, |p| {
        let nat = p.name_and_type("x", "I");
        p.u2u2_entry(tag::DYNAMIC, 0, nat)
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn invoke_dynamic_bootstrap_index_out_of_range_rejected() {
    // bootstrap_method_attr_index 1 with only one bootstrap method (index 0).
    // It indexes the BootstrapMethods table, not the constant pool, so an
    // out-of-range one is a plain format check.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let bsm = cb.pool.bootstrap_handle();
    let nat = cb.pool.name_and_type("x", "()V");
    cb.pool.u2u2_entry(tag::INVOKE_DYNAMIC, 1, nat);
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn invoke_dynamic_with_bootstrap_methods_accepted() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let bsm = cb.pool.bootstrap_handle();
    let nat = cb.pool.name_and_type("x", "()V");
    cb.pool.u2u2_entry(tag::INVOKE_DYNAMIC, 1, nat);
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 2);
    assert_parses(&cb.to_bytes(), |_| ());
}

#[test]
fn invoke_dynamic_with_field_descriptor_rejected() {
    // An invokedynamic call site names a method, so its descriptor is a method
    // descriptor (JVMS 4.4.10).
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let bsm = cb.pool.bootstrap_handle();
    let nat = cb.pool.name_and_type("x", "I");
    cb.pool.u2u2_entry(tag::INVOKE_DYNAMIC, 0, nat);
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn dynamic_with_method_descriptor_rejected() {
    // A dynamically computed constant names a value, so its descriptor is a
    // field descriptor (JVMS 4.4.10).
    let mut cb = ClassBuilder::new(55);
    let bsm = cb.pool.bootstrap_handle();
    let nat = cb.pool.name_and_type("x", "()V");
    cb.pool.u2u2_entry(tag::DYNAMIC, 0, nat);
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn invoke_dynamic_name_and_type_index_zero_rejected() {
    // The BootstrapMethods attribute is present, so only the index is at fault.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let bsm = cb.pool.bootstrap_handle();
    cb.pool.u2u2_entry(tag::INVOKE_DYNAMIC, 0, 0);
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn invoke_dynamic_name_and_type_index_not_name_and_type_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let bsm = cb.pool.bootstrap_handle();
    let not_a_nat = cb.pool.utf8("x");
    cb.pool.u2u2_entry(tag::INVOKE_DYNAMIC, 0, not_a_nat);
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);
    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn dynamic_name_and_type_index_pointing_at_unusable_slot_rejected() {
    let mut cb = ClassBuilder::new(55);
    let bsm = cb.pool.bootstrap_handle();
    let l = cb.pool.long(1);
    cb.pool.u2u2_entry(tag::DYNAMIC, 0, l + 1);
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn dynamic_name_and_type_index_not_name_and_type_rejected() {
    let mut cb = ClassBuilder::new(55);
    let bsm = cb.pool.bootstrap_handle();
    let not_a_nat = cb.pool.utf8("x");
    cb.pool.u2u2_entry(tag::DYNAMIC, 0, not_a_nat);
    cb.reserve_attributes(1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, bsm, 1);
    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

// ---------------------------------------------------------------------------
// Entries only valid from a given version (table 4.4-B)
// ---------------------------------------------------------------------------

#[test]
fn method_handle_requires_version_51() {
    assert_rejected_with!(
        &class_with_tag_at_version(tag::METHOD_HANDLE, 50),
        ClassParserError::ClassParseInvalidFeatureUsedForVersion
    );
    assert_parses(&class_with_tag_at_version(tag::METHOD_HANDLE, 51), |_| ());
}

#[test]
fn method_type_requires_version_51() {
    assert_rejected_with!(
        &class_with_tag_at_version(tag::METHOD_TYPE, 50),
        ClassParserError::ClassParseInvalidFeatureUsedForVersion
    );
    assert_parses(&class_with_tag_at_version(tag::METHOD_TYPE, 51), |_| ());
}

#[test]
fn invoke_dynamic_requires_version_51() {
    assert_rejected_with!(
        &class_with_tag_at_version(tag::INVOKE_DYNAMIC, 50),
        ClassParserError::ClassParseInvalidFeatureUsedForVersion
    );
    assert_parses(&class_with_tag_at_version(tag::INVOKE_DYNAMIC, 51), |_| ());
}

#[test]
fn dynamic_requires_version_55() {
    assert_rejected_with!(
        &class_with_tag_at_version(tag::DYNAMIC, 54),
        ClassParserError::ClassParseInvalidFeatureUsedForVersion
    );
    assert_parses(&class_with_tag_at_version(tag::DYNAMIC, 55), |_| ());
}

#[test]
fn module_entry_in_ordinary_class_rejected() {
    // CONSTANT_Module and CONSTANT_Package are only valid in module-info.
    let (bytes, _) = class_with(53, |p| p.module("java.base"));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn package_entry_in_ordinary_class_rejected() {
    let (bytes, _) = class_with(53, |p| p.package("java/lang"));
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_and_package_entries_accepted_in_module_info() {
    assert_parses(&module_info_class(53, Some("com/example/api")), |_| ());
}

#[test]
fn module_entry_requires_version_53() {
    // CONSTANT_Module arrived with modules in Java 9.
    assert_rejected_with!(
        &module_info_class(52, None),
        ClassParserError::ClassParseInvalidFeatureUsedForVersion
    );
}
