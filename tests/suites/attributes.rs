//! Attributes (JVMS 4.7): the attribute header, the rule that unknown
//! attributes are skipped, `attribute_length` agreeing with the structure, and
//! each attribute this parser understands. StackMapTable (4.7.4), the
//! annotation attributes (4.7.16 - 4.7.22) and the Module attributes
//! (4.7.25 - 4.7.27) have their own files.
//!
//! Ported from `c_backup/tests/test_classfile_attributes.c`. The C suite could
//! read a single attribute in isolation; here every case is driven through
//! `ClassFile::parse`, so each attribute is put where the JVMS allows it - on
//! the class, a field, a method, inside Code, or on a record component.

use crate::common::*;

use bytecode_vm::parser::class_file::{
    AttributeInfo, ClassParserError, InnerClassAccessFlags, MethodParameterAccessFlags,
};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// A class whose class-level attributes are exactly the `count` attributes
/// `write` emits.
fn class_attrs(
    major: u16,
    count: u16,
    write: impl FnOnce(&mut Bytes, &mut PoolBuilder),
) -> Vec<u8> {
    let mut cb = ClassBuilder::new(major);
    cb.reserve_attributes(count);
    write(&mut cb.attributes, &mut cb.pool);
    cb.to_bytes()
}

/// A class carrying one class-level attribute at the default version.
fn class_with_attr(write: impl FnOnce(&mut Bytes, &mut PoolBuilder)) -> Vec<u8> {
    class_attrs(DEFAULT_MAJOR_VERSION, 1, write)
}

/// A class with one `private static final int f` carrying `count` attributes.
fn field_with_attrs(count: u16, write: impl FnOnce(&mut Bytes, &mut PoolBuilder)) -> Vec<u8> {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_field(acc::PRIVATE | acc::STATIC | acc::FINAL, "f", "I", count);
    write(&mut cb.fields, &mut cb.pool);
    cb.to_bytes()
}

/// A class with one method carrying `count` attributes. The method is native so
/// that it is allowed to have no Code attribute (JVMS 4.7.3).
fn native_method_with_attrs(
    count: u16,
    write: impl FnOnce(&mut Bytes, &mut PoolBuilder),
) -> Vec<u8> {
    native_method_with_attrs_desc("()V", count, write)
}

/// As above, with a chosen descriptor - MethodParameters has to agree with the
/// parameter list.
fn native_method_with_attrs_desc(
    desc: &str,
    count: u16,
    write: impl FnOnce(&mut Bytes, &mut PoolBuilder),
) -> Vec<u8> {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC | acc::STATIC | acc::NATIVE, "m", desc, count);
    write(&mut cb.methods, &mut cb.pool);
    cb.to_bytes()
}

/// A class with one concrete method whose only attribute is a Code attribute
/// built by `write`, which is handed the method's byte buffer positioned right
/// after `attributes_count`.
fn method_with_code(write: impl FnOnce(&mut Bytes, &mut PoolBuilder)) -> Vec<u8> {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC | acc::STATIC, "m", "()V", 1);
    write(&mut cb.methods, &mut cb.pool);
    cb.to_bytes()
}

/// Sixteen bytes of code, so nested tables have plenty of valid `start_pc`s.
const FILLER_CODE_LEN: u32 = 16;

/// A class with one method whose Code holds exactly `count` sub-attributes.
/// The code is 15 nops and a return, with four local slots, so nested tables
/// can use any pc below 16 and any slot below 4.
fn code_with_attrs(count: u16, write: impl FnOnce(&mut Bytes, &mut PoolBuilder)) -> Vec<u8> {
    method_with_code(|m, p| {
        let code_name = p.utf8("Code");
        let body = m.attr_begin(code_name);
        m.u2(2); // max_stack
        m.u2(4); // max_locals
        m.u4(FILLER_CODE_LEN);
        m.fill(op::NOP, FILLER_CODE_LEN as usize - 1);
        m.u1(op::RETURN);
        m.u2(0); // exception_table_length
        m.u2(count);
        write(m, p);
        m.attr_end(body);
    })
}

/// A `public final class Point extends java.lang.Record` with a private final
/// field per component, carrying one class-level attribute from `write`.
fn record_class(
    components: &[(&str, &str)],
    write: impl FnOnce(&mut Bytes, &mut PoolBuilder),
) -> Vec<u8> {
    let mut cb = ClassBuilder::empty(60);
    cb.access_flags = acc::PUBLIC | acc::SUPER | acc::FINAL;
    cb.this_class = cb.pool.class("Point");
    cb.super_class = cb.pool.class("java/lang/Record");
    for (name, desc) in components {
        cb.add_field(acc::PRIVATE | acc::FINAL, name, desc, 0);
    }
    cb.reserve_attributes(1);
    write(&mut cb.attributes, &mut cb.pool);
    cb.to_bytes()
}

/// The constant kind each attribute's leading index wants, so that a
/// length-only test does not accidentally also trip the wrong-kind check.
fn body_target(pool: &mut PoolBuilder, name: &str) -> u16 {
    match name {
        "NestHost" => pool.class("Outer"),
        "ConstantValue" => pool.integer(1),
        _ => pool.utf8("x"),
    }
}

/// Writes `name` with a hand-chosen `attribute_length` over a `body_len`-byte
/// body, so the two can be made to disagree. The body's first u2, if there is
/// one, names a constant of the kind the attribute expects, leaving the length
/// as the only fault.
fn attr_with_length(
    out: &mut Bytes,
    pool: &mut PoolBuilder,
    name: &str,
    length: u32,
    body_len: usize,
) {
    let idx = pool.utf8(name);
    let target = body_target(pool, name);
    out.u2(idx);
    out.u4(length);
    let body = out.len();
    out.fill(0, body_len);
    if body_len >= 2 {
        out.patch_u2(body, target);
    }
}

/// The one attribute in `attrs`, with a clearer message than indexing.
#[track_caller]
fn only<'a, 'b>(attrs: &'b [AttributeInfo<'a>]) -> &'b AttributeInfo<'a> {
    assert_eq!(attrs.len(), 1, "expected exactly one attribute");
    &attrs[0]
}

/// Matches one attribute against a variant, panicking with the variant's name
/// if it is something else. `AttributeInfo` has no `Debug`, so the message can
/// only name what was wanted.
macro_rules! expect_attr {
    ($attr:expr, $pat:pat => $out:expr) => {
        match $attr {
            $pat => $out,
            _ => panic!("expected {}", stringify!($pat)),
        }
    };
}

/// The first attribute in a list matching a variant. The parser is free to keep
/// attributes in any order, so lookups go by variant rather than by position.
macro_rules! find_attr {
    ($attrs:expr, $pat:pat => $out:expr) => {
        $attrs
            .iter()
            .find_map(|a| match a {
                $pat => Some($out),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no attribute matching {}", stringify!($pat)))
    };
}

// ---------------------------------------------------------------------------
// Attribute names (JVMS 4.7.1)
//
// The C suite tested `attribute_tag_from_name` directly; name recognition is
// now internal to parsing, so the same ground is covered by checking which
// variant a name parses to.
// ---------------------------------------------------------------------------

#[test]
fn class_level_attribute_names_map_to_their_variants() {
    // Method- and Code-level names are covered by their own sections below.
    let bytes = class_with_attr(|a, p| {
        let n = p.utf8("SourceFile");
        let f = p.utf8("Test.java");
        a.attr_u2(n, f);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::SourceFileIndex { .. } => ())
    });

    let bytes = class_with_attr(|a, p| {
        let n = p.utf8("Signature");
        let s = p.utf8("Ljava/lang/Object;");
        a.attr_u2(n, s);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::Signature { .. } => ())
    });

    let bytes = class_with_attr(|a, p| {
        let n = p.utf8("Synthetic");
        a.attr_empty(n);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::Synthetic => ())
    });

    let bytes = class_with_attr(|a, p| {
        let n = p.utf8("Deprecated");
        a.attr_empty(n);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::Deprecated => ())
    });

    let bytes = class_with_attr(|a, p| {
        let n = p.utf8("SourceDebugExtension");
        a.attr_raw(n, b"SMAP");
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::SourceDebugExtension { .. } => ())
    });

    let bytes = class_with_attr(|a, p| {
        let n = p.utf8("InnerClasses");
        let body = a.attr_begin(n);
        a.u2(0);
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::InnerClasses { .. } => ())
    });

    let bytes = class_with_attr(|a, p| {
        let n = p.utf8("EnclosingMethod");
        let outer = p.class("Outer");
        let body = a.attr_begin(n);
        a.u2(outer);
        a.u2(0);
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::EnclosingMethod { .. } => ())
    });

    let bytes = class_attrs(55, 1, |a, p| {
        let n = p.utf8("NestHost");
        let host = p.class("Outer");
        a.attr_u2(n, host);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::NestHostClass { .. } => ())
    });

    let bytes = class_attrs(55, 1, |a, p| {
        let n = p.utf8("NestMembers");
        let m = p.class("Test$Inner");
        a.attr_u2_table(n, &[m]);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::NestMembers { .. } => ())
    });

    let bytes = class_attrs(61, 1, |a, p| {
        let n = p.utf8("PermittedSubclasses");
        let s = p.class("Test$Sub");
        a.attr_u2_table(n, &[s]);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::PermittedSubclasses { .. } => ())
    });

    let bytes = class_attrs(DEFAULT_MAJOR_VERSION, 1, |a, p| {
        let handle = p.bootstrap_handle();
        ClassBuilder::write_bootstrap_methods(a, p, handle, 1);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::BootstrapMethods { .. } => ())
    });

    let bytes = record_class(&[], |a, p| {
        let n = p.utf8("Record");
        let body = a.attr_begin(n);
        a.u2(0);
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::Record { .. } => ())
    });
}

#[test]
fn near_miss_attribute_names_are_unknown() {
    // Names are matched byte for byte: no trimming, no case folding, no
    // prefix match (JVMS 4.7.1).
    for name in [
        "",
        "code",
        "CODE",
        "Code ",
        " Code",
        "CodeX",
        "Cod",
        "SourceFileX",
        "Module\n",
        "org.example.Custom",
        "RuntimeVisibleAnnotation",
    ] {
        let bytes = class_with_attr(|a, p| {
            let n = p.utf8(name);
            a.attr_raw(n, b"\x00\x01");
        });
        assert_parses(&bytes, |cf| {
            let got = expect_attr!(only(&cf.attributes), AttributeInfo::Unknown { name, .. } => *name);
            assert_utf8_bytes_eq(got, name.as_bytes());
        });
    }
}

// ---------------------------------------------------------------------------
// Attribute header
// ---------------------------------------------------------------------------

#[test]
fn attribute_name_index_zero_rejected() {
    // Index 0 is never a constant pool entry (JVMS 4.4).
    let bytes = class_with_attr(|a, p| {
        p.utf8("SourceFile");
        a.attr_u2(0, 1);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn attribute_name_index_past_pool_rejected() {
    let bytes = class_with_attr(|a, p| {
        let sf = p.utf8("SourceFile");
        a.attr_u2(sf + 1, 1);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn attribute_name_index_in_unusable_long_slot_rejected() {
    // A Long takes two slots and the second one names no entry at all
    // (JVMS 4.4.5), so it is out of range rather than the wrong kind.
    let bytes = class_with_attr(|a, p| {
        p.utf8("SourceFile");
        let l = p.long(1);
        a.attr_u2(l + 1, 1);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn attribute_name_index_not_utf8_rejected() {
    // The name has to be read as a string, so a numeric entry must be caught
    // rather than reinterpreted.
    let bytes = class_with_attr(|a, p| {
        let i = p.integer(0x4141_4141);
        a.attr_u2(i, 1);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn attribute_name_index_pointing_at_class_rejected() {
    // A CONSTANT_Class holding "Code" is still not a Utf8 entry.
    let bytes = class_with_attr(|a, p| {
        let c = p.class("Code");
        a.attr_u2(c, 1);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn attribute_header_truncated_rejected() {
    let bytes = class_with_attr(|a, p| {
        let sf = p.utf8("SourceFile");
        a.u2(sf);
        a.u2(0); // half of attribute_length
    });
    assert_rejected_with!(&bytes, ClassParserError::Truncated(_));
}

#[test]
fn attribute_length_is_unsigned_32_bit() {
    // A length with the top bit set must not be read as a negative count and
    // then quietly accepted.
    let bytes = class_with_attr(|a, p| {
        let unk = p.utf8("Vendor");
        a.u2(unk);
        a.u4(0x8000_0000);
        a.fill(0, 16);
    });
    // Two gigabytes of body are not there, so the file runs out first.
    assert_rejected_with!(&bytes, ClassParserError::Truncated(_));
}

// ---------------------------------------------------------------------------
// Unknown attributes are skipped (JVMS 4.7.1)
// ---------------------------------------------------------------------------

#[test]
fn unknown_attribute_skipped() {
    let body: &[u8] = b"\xde\xad\xbe\xef\x00\x01";
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("org.example.Custom");
        a.attr_raw(name, body);
    });
    assert_parses(&bytes, |cf| {
        let (name, info) =
            expect_attr!(only(&cf.attributes), AttributeInfo::Unknown { name, info } => (*name, *info));
        assert_utf8_eq(name, "org.example.Custom");
        assert_eq!(info, body, "the raw body must be kept verbatim");
    });
}

#[test]
fn unknown_attribute_empty_skipped() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("Empty");
        a.attr_raw(name, b"");
    });
    assert_parses(&bytes, |cf| {
        let info = expect_attr!(only(&cf.attributes), AttributeInfo::Unknown { info, .. } => *info);
        assert!(info.is_empty());
    });
}

#[test]
fn unknown_attribute_large_body_skipped() {
    // A body far longer than any structure the parser knows must still just be
    // stepped over.
    let mut body = Bytes::new();
    body.fill(0x5A, 100_000);
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("Big");
        a.attr_raw(name, &body.data);
    });
    assert_parses(&bytes, |cf| {
        let info = expect_attr!(only(&cf.attributes), AttributeInfo::Unknown { info, .. } => *info);
        assert_eq!(info.len(), 100_000);
    });
}

#[test]
fn unknown_attribute_then_known_attribute() {
    // Skipping must land exactly on the next attribute, not a byte off.
    let bytes = class_attrs(DEFAULT_MAJOR_VERSION, 2, |a, p| {
        let unk = p.utf8("Whatever");
        let sf = p.utf8("SourceFile");
        let file = p.utf8("A.java");
        a.attr_raw(unk, b"\x00\x01\x02");
        a.attr_u2(sf, file);
    });
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.attributes.len(), 2);
        expect_attr!(&cf.attributes[0], AttributeInfo::Unknown { .. } => ());
        let file =
            expect_attr!(&cf.attributes[1], AttributeInfo::SourceFileIndex { value } => *value);
        assert_utf8_eq(file, "A.java");
    });
}

#[test]
fn unknown_attribute_body_truncated_rejected() {
    // Skipping is bounded by the file: a body that runs off the end is an error.
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("Whatever");
        a.u2(name);
        a.u4(10);
        a.raw(b"abc");
    });
    assert_rejected_with!(&bytes, ClassParserError::Truncated(_));
}

#[test]
fn unknown_attributes_skipped_everywhere_in_class() {
    // Every attribute table in the file has to honour 4.7.1, not just the
    // class-level one.
    const JUNK: &[u8] = &[1, 2, 3, 4, 5, 6, 7];
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let custom = cb.pool.utf8("com.example.Custom");
    let code_name = cb.pool.utf8("Code");

    cb.add_field(acc::PRIVATE, "f", "I", 1);
    cb.fields.attr_raw(custom, JUNK);

    cb.add_method(acc::PUBLIC | acc::STATIC, "m", "()V", 2);
    cb.methods.attr_raw(custom, JUNK);
    let body = cb.methods.attr_begin(code_name);
    cb.methods.u2(0);
    cb.methods.u2(0);
    cb.methods.u4(1);
    cb.methods.u1(op::RETURN);
    cb.methods.u2(0); // exception_table_length
    cb.methods.u2(1); // attributes_count
    cb.methods.attr_raw(custom, JUNK); // inside Code
    cb.methods.attr_end(body);

    cb.reserve_attributes(1);
    cb.attributes.attr_raw(custom, JUNK);

    let bytes = cb.to_bytes();
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::Unknown { .. } => ());
        expect_attr!(only(&cf.fields[0].attributes), AttributeInfo::Unknown { .. } => ());
        let code = find_attr!(cf.methods[0].attributes, AttributeInfo::Code { attributes, .. } => attributes);
        expect_attr!(only(code), AttributeInfo::Unknown { .. } => ());
        find_attr!(cf.methods[0].attributes, AttributeInfo::Unknown { .. } => ());
    });
}

#[test]
fn unknown_attribute_named_by_non_ascii_utf8_skipped() {
    // The name is compared as bytes, so a name that is not ASCII is simply
    // unknown rather than a decoding failure.
    let bytes = class_with_attr(|a, p| {
        let idx = p.utf8_bytes(&[0xC3, 0xA9, b'x']);
        a.attr_raw(idx, b"zz");
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::Unknown { .. } => ())
    });
}

// ---------------------------------------------------------------------------
// attribute_length must match the structure (JVMS 4.7)
// ---------------------------------------------------------------------------

#[test]
fn fixed_size_attributes_with_wrong_length_rejected() {
    for name in ["Signature", "SourceFile", "NestHost"] {
        // Two bytes left over once the single u2 has been read...
        let longer = class_attrs(55, 1, |a, p| attr_with_length(a, p, name, 4, 4));
        assert_rejected_with!(&longer, ClassParserError::ClassParseInvalidAttributeLength);
        // ...and a length that cannot even hold that u2.
        let shorter = class_attrs(55, 1, |a, p| attr_with_length(a, p, name, 1, 2));
        assert_rejected_with!(&shorter, ClassParserError::ClassParseInvalidAttributeLength);
    }
    // ConstantValue is a field attribute (JVMS 4.7.2).
    let longer = field_with_attrs(1, |f, p| attr_with_length(f, p, "ConstantValue", 4, 4));
    assert_rejected_with!(&longer, ClassParserError::ClassParseInvalidAttributeLength);
    let shorter = field_with_attrs(1, |f, p| attr_with_length(f, p, "ConstantValue", 1, 2));
    assert_rejected_with!(&shorter, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn synthetic_and_deprecated_with_nonzero_length_rejected() {
    // Both are marker attributes: attribute_length must be 0, so any body at
    // all is bytes the structure cannot account for.
    assert_rejected_with!(
        &class_with_attr(|a, p| attr_with_length(a, p, "Synthetic", 2, 2)),
        ClassParserError::ClassParseInvalidAttributeLength
    );
    assert_rejected_with!(
        &class_with_attr(|a, p| attr_with_length(a, p, "Deprecated", 2, 2)),
        ClassParserError::ClassParseInvalidAttributeLength
    );
}

#[test]
fn enclosing_method_with_wrong_length_rejected() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("EnclosingMethod");
        let c = p.class("Outer");
        let body = a.attr_begin(name);
        a.u2(c);
        a.u2(0);
        a.u2(0); // one u2 more than the structure has
        a.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn table_attribute_length_disagreeing_with_count_rejected() {
    // Exceptions says length 6 (count plus two entries) but the count claims
    // one entry, leaving two bytes unaccounted for.
    let bytes = native_method_with_attrs(1, |m, p| {
        let name = p.utf8("Exceptions");
        let c = p.class("java/lang/Exception");
        m.u2(name);
        m.u4(6);
        m.u2(1);
        m.u2(c);
        m.u2(c);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn table_attribute_length_too_short_for_count_rejected() {
    // The mirror image: the count needs four bytes of entries that the length
    // does not reach. The entries are written all the same, so the disagreement
    // is with the declared length rather than with the end of the file.
    let bytes = native_method_with_attrs(1, |m, p| {
        let name = p.utf8("Exceptions");
        let c = p.class("java/lang/Exception");
        m.u2(name);
        m.u4(2); // room for the count alone
        m.u2(2);
        m.u2(c);
        m.u2(c);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn code_attribute_length_too_long_rejected() {
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(0);
        m.u2(0);
        m.u4(1);
        m.u1(op::RETURN);
        m.u2(0);
        m.u2(0);
        m.u1(0); // one byte the structure does not account for
        m.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn code_attribute_length_too_short_rejected() {
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(0);
        m.u2(0);
        m.u4(1);
        m.u1(op::RETURN);
        m.u2(0);
        m.u2(0);
        m.attr_end(body);
        m.patch_u4(body - 4, 11); // the real body is 13 bytes
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

// ---------------------------------------------------------------------------
// ConstantValue (4.7.2)
// ---------------------------------------------------------------------------

#[test]
fn constant_value_attribute() {
    let bytes = field_with_attrs(1, |f, p| {
        let name = p.utf8("ConstantValue");
        let v = p.integer(7);
        f.attr_u2(name, v);
    });
    assert_parses(&bytes, |cf| {
        let value = expect_attr!(
            only(&cf.fields[0].attributes),
            AttributeInfo::ConstantValueIndex { value } => *value
        );
        // The index is kept as-is: which constant kind is allowed depends on
        // the field's descriptor.
        assert!(matches!(
            cf.constant(value),
            Some(bytecode_vm::parser::class_file::ConstantPoolEntry::Integer(7))
        ));
    });
}

// ---------------------------------------------------------------------------
// Code (4.7.3)
// ---------------------------------------------------------------------------

#[test]
fn code_attribute_full() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let exc = cb.pool.class("java/lang/Exception");
    let this_name = cb.pool.utf8("this");
    let this_desc = cb.pool.utf8("LTest;");
    let code_name = cb.pool.utf8("Code");
    let lnt_name = cb.pool.utf8("LineNumberTable");
    let lvt_name = cb.pool.utf8("LocalVariableTable");
    let ctor = cb.pool.methodref("java/lang/Object", "<init>", "()V");
    let code = [
        op::ALOAD_0,
        op::INVOKESPECIAL,
        (ctor >> 8) as u8,
        ctor as u8,
        op::NOP,
        op::RETURN,
    ];

    cb.add_method(acc::PUBLIC, "<init>", "()V", 1);
    let body = cb.methods.attr_begin(code_name);
    cb.methods.u2(2); // max_stack
    cb.methods.u2(3); // max_locals
    cb.methods.u4(code.len() as u32);
    cb.methods.raw(&code);
    cb.methods.u2(2); // exception_table_length
    cb.methods.u2(0);
    cb.methods.u2(4);
    cb.methods.u2(4);
    cb.methods.u2(exc);
    cb.methods.u2(1);
    cb.methods.u2(5);
    cb.methods.u2(5);
    cb.methods.u2(0); // catch_type 0 means "any", as finally uses
    cb.methods.u2(2); // attributes_count
    let lnt = cb.methods.attr_begin(lnt_name);
    cb.methods.u2(2);
    cb.methods.u2(0);
    cb.methods.u2(10);
    cb.methods.u2(4);
    cb.methods.u2(11);
    cb.methods.attr_end(lnt);
    let lvt = cb.methods.attr_begin(lvt_name);
    cb.methods.u2(1);
    cb.methods.u2(0);
    cb.methods.u2(6);
    cb.methods.u2(this_name);
    cb.methods.u2(this_desc);
    cb.methods.u2(0);
    cb.methods.attr_end(lvt);
    cb.methods.attr_end(body);

    let bytes = cb.to_bytes();
    assert_parses(&bytes, |cf| {
        let (max_stack, max_locals, parsed_code, table, attrs) = expect_attr!(
            only(&cf.methods[0].attributes),
            AttributeInfo::Code { max_stack, max_locals, code, exception_table, attributes } =>
                (*max_stack, *max_locals, *code, exception_table, attributes)
        );
        assert_eq!(max_stack, 2);
        assert_eq!(max_locals, 3);
        assert_eq!(parsed_code, &code[..]);

        assert_eq!(table.len(), 2);
        assert_eq!(table[0].start_pc, 0);
        assert_eq!(table[0].end_pc, 4);
        assert_eq!(table[0].handler_pc, 4);
        assert_utf8_eq(table[0].catch_type.expect("a named catch type"), "java/lang/Exception");
        assert_eq!(table[1].start_pc, 1);
        assert_eq!(table[1].end_pc, 5);
        assert_eq!(table[1].handler_pc, 5);
        assert!(table[1].catch_type.is_none(), "catch_type 0 means any exception");

        assert_eq!(attrs.len(), 2);
        let lines = find_attr!(attrs, AttributeInfo::LineNumberTable { entries } => entries);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].start_pc, 4);
        assert_eq!(lines[1].line_number, 11);
        let locals = find_attr!(attrs, AttributeInfo::LocalVariableTable { entries } => entries);
        assert_utf8_eq(locals[0].name, "this");
    });
}

#[test]
fn code_attribute_code_length_zero_rejected() {
    // code_length must be greater than zero (JVMS 4.7.3).
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(0);
        m.u2(0);
        m.u4(0);
        m.u2(0);
        m.u2(0);
        m.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidCodeAttribute);
}

#[test]
fn code_attribute_code_length_65536_rejected() {
    // code_length must be less than 65536, so branch offsets stay in range.
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(0);
        m.u2(0);
        m.u4(65536);
        m.fill(op::NOP, 65535);
        m.u1(op::RETURN);
        m.u2(0);
        m.u2(0);
        m.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidCodeAttribute);
}

#[test]
fn code_attribute_code_length_65535_accepted() {
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(0);
        m.u2(0);
        m.u4(65535);
        m.fill(op::NOP, 65534);
        m.u1(op::RETURN);
        m.u2(0);
        m.u2(0);
        m.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let code = expect_attr!(
            only(&cf.methods[0].attributes),
            AttributeInfo::Code { code, .. } => *code
        );
        assert_eq!(code.len(), 65535);
        assert_eq!(code[65534], op::RETURN);
    });
}

#[test]
fn code_max_stack_and_max_locals_at_their_maximum_accepted() {
    // Both are u2, so 65535 is the largest legal value and must not be treated
    // as a sentinel.
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(65535);
        m.u2(65535);
        m.u4(1);
        m.u1(op::RETURN);
        m.u2(0);
        m.u2(0);
        m.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let (max_stack, max_locals) = expect_attr!(
            only(&cf.methods[0].attributes),
            AttributeInfo::Code { max_stack, max_locals, .. } => (*max_stack, *max_locals)
        );
        assert_eq!(max_stack, 65535);
        assert_eq!(max_locals, 65535);
    });
}

#[test]
fn code_attribute_code_bytes_past_eof_rejected() {
    // code_length promises 90 bytes; only 10 are there.
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        m.u2(name);
        m.u4(100);
        m.u2(0);
        m.u2(0);
        m.u4(90);
        m.fill(op::NOP, 10);
    });
    // The attribute's own 100 bytes already outrun the file.
    assert_rejected_with!(&bytes, ClassParserError::Truncated(_));
}

#[test]
fn code_attribute_nested_stack_map_table() {
    // Code is the only place StackMapTable may appear; the frames themselves
    // are covered in the stack map tests.
    let bytes = method_with_code(|m, p| {
        let code_name = p.utf8("Code");
        let smt_name = p.utf8("StackMapTable");
        let body = m.attr_begin(code_name);
        m.u2(1);
        m.u2(1);
        m.u4(2);
        m.u1(op::NOP);
        m.u1(op::RETURN);
        m.u2(0);
        m.u2(1);
        let smt = m.attr_begin(smt_name);
        m.u2(1);
        m.u1(1); // same_frame, offset_delta 1
        m.attr_end(smt);
        m.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let attrs = expect_attr!(
            only(&cf.methods[0].attributes),
            AttributeInfo::Code { attributes, .. } => attributes
        );
        let frames = expect_attr!(only(attrs), AttributeInfo::StackMapTable { entries } => entries);
        assert_eq!(frames.len(), 1);
    });
}

#[test]
fn exception_table_entry_fields() {
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let exc = p.class("java/lang/RuntimeException");
        let body = m.attr_begin(name);
        m.u2(1);
        m.u2(0);
        m.u4(6);
        m.fill(op::NOP, 5);
        m.u1(op::RETURN);
        m.u2(1);
        m.u2(1);
        m.u2(2);
        m.u2(3);
        m.u2(exc);
        m.u2(0);
        m.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let table = expect_attr!(
            only(&cf.methods[0].attributes),
            AttributeInfo::Code { exception_table, .. } => exception_table
        );
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].start_pc, 1);
        assert_eq!(table[0].end_pc, 2);
        assert_eq!(table[0].handler_pc, 3);
        assert_utf8_eq(table[0].catch_type.expect("a named catch type"), "java/lang/RuntimeException");
    });
}

#[test]
fn code_attribute_exception_table_overruns_length_rejected() {
    // The handlers the count promises reach past the end of the attribute,
    // which is a length disagreement rather than a short file.
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(0);
        m.u2(0);
        m.u4(1);
        m.u1(op::RETURN);
        m.u2(3); // claims three handlers, provides one
        m.u2(0);
        m.u2(1);
        m.u2(0);
        m.u2(0);
        m.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn exception_table_entry_overruns_length_rejected() {
    // One handler, and the entry itself stops mid-way through the attribute.
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(0);
        m.u2(0);
        m.u4(1);
        m.u1(op::RETURN);
        m.u2(1);
        m.u2(0);
        m.u2(1);
        m.u1(0); // half of handler_pc, and no catch_type at all
        m.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn code_attribute_truncated_at_every_offset_rejected() {
    // Every structure inside Code is length-driven, so a cut anywhere has to
    // surface as an error rather than a short read that is silently accepted.
    let bytes = method_with_code(|m, p| {
        let code_name = p.utf8("Code");
        let lnt_name = p.utf8("LineNumberTable");
        let body = m.attr_begin(code_name);
        m.u2(1);
        m.u2(1);
        m.u4(3);
        m.raw(&[op::NOP, op::NOP, op::RETURN]);
        m.u2(1);
        m.u2(0);
        m.u2(2);
        m.u2(2);
        m.u2(0);
        m.u2(1);
        let lnt = m.attr_begin(lnt_name);
        m.u2(1);
        m.u2(0);
        m.u2(1);
        m.attr_end(lnt);
        m.attr_end(body);
    });
    for len in 0..bytes.len() {
        with_parsed(&bytes[..len], |result| {
            assert!(
                result.is_err(),
                "class cut to {len} of {} bytes was accepted",
                bytes.len()
            );
        });
    }
}

#[test]
fn code_exception_handler_ranges_validated() {
    // start_pc < end_pc <= code_length (JVMS 4.7.3); here start == end.
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(1);
        m.u2(0);
        m.u4(2);
        m.u1(op::NOP);
        m.u1(op::RETURN);
        m.u2(1);
        m.u2(1);
        m.u2(1);
        m.u2(0);
        m.u2(0);
        m.u2(0);
        m.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidCodeAttribute);
}

#[test]
fn code_exception_handler_pc_past_code_rejected() {
    // handler_pc must be a valid index into code (JVMS 4.7.3).
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let body = m.attr_begin(name);
        m.u2(1);
        m.u2(0);
        m.u4(2);
        m.u1(op::NOP);
        m.u1(op::RETURN);
        m.u2(1);
        m.u2(0);
        m.u2(2);
        m.u2(2); // one past the last instruction
        m.u2(0);
        m.u2(0);
        m.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidCodeAttribute);
}

#[test]
fn code_exception_catch_type_must_be_class() {
    // catch_type is a CONSTANT_Class index, not the name itself.
    let bytes = method_with_code(|m, p| {
        let name = p.utf8("Code");
        let not_class = p.utf8("java/lang/Exception");
        let body = m.attr_begin(name);
        m.u2(1);
        m.u2(0);
        m.u4(2);
        m.u1(op::NOP);
        m.u1(op::RETURN);
        m.u2(1);
        m.u2(0);
        m.u2(1);
        m.u2(1);
        m.u2(not_class);
        m.u2(0);
        m.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

// ---------------------------------------------------------------------------
// Exceptions (4.7.5)
// ---------------------------------------------------------------------------

#[test]
fn exceptions_attribute() {
    let bytes = native_method_with_attrs(1, |m, p| {
        let name = p.utf8("Exceptions");
        let a1 = p.class("java/io/IOException");
        let a2 = p.class("java/lang/InterruptedException");
        let a3 = p.class("java/sql/SQLException");
        m.attr_u2_table(name, &[a1, a2, a3]);
    });
    assert_parses(&bytes, |cf| {
        let entries = expect_attr!(
            only(&cf.methods[0].attributes),
            AttributeInfo::Exceptions { entries } => entries
        );
        assert_eq!(entries.len(), 3);
        assert_utf8_eq(entries[0], "java/io/IOException");
        assert_utf8_eq(entries[1], "java/lang/InterruptedException");
        assert_utf8_eq(entries[2], "java/sql/SQLException");
    });
}

#[test]
fn exceptions_attribute_empty() {
    // A method that throws nothing may still carry the attribute.
    let bytes = native_method_with_attrs(1, |m, p| {
        let name = p.utf8("Exceptions");
        m.attr_u2_table(name, &[]);
    });
    assert_parses(&bytes, |cf| {
        let entries = expect_attr!(
            only(&cf.methods[0].attributes),
            AttributeInfo::Exceptions { entries } => entries
        );
        assert!(entries.is_empty());
    });
}

#[test]
fn exceptions_entry_must_be_class() {
    let bytes = native_method_with_attrs(1, |m, p| {
        let name = p.utf8("Exceptions");
        let not_class = p.utf8("java/io/IOException");
        m.attr_u2_table(name, &[not_class]);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn exceptions_entry_past_pool_rejected() {
    // The companion to the case above: an index that names nothing at all is a
    // different fault from one that names the wrong kind of entry.
    let bytes = native_method_with_attrs(1, |m, p| {
        let name = p.utf8("Exceptions");
        let past_end = p.next;
        m.attr_u2_table(name, &[past_end]);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

// ---------------------------------------------------------------------------
// InnerClasses (4.7.6) / EnclosingMethod (4.7.7)
// ---------------------------------------------------------------------------

#[test]
fn inner_classes_attribute() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("InnerClasses");
        let inner = p.class("Outer$Inner");
        let outer = p.class("Outer");
        let simple = p.utf8("Inner");
        let anon = p.class("Outer$1");
        let body = a.attr_begin(name);
        a.u2(2);
        a.u2(inner);
        a.u2(outer);
        a.u2(simple);
        a.u2(acc::PRIVATE | acc::STATIC);
        // An anonymous class has neither an enclosing class nor a simple name.
        a.u2(anon);
        a.u2(0);
        a.u2(0);
        a.u2(acc::FINAL | acc::SYNTHETIC);
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let classes =
            expect_attr!(only(&cf.attributes), AttributeInfo::InnerClasses { value } => value);
        assert_eq!(classes.len(), 2);
        assert_utf8_eq(classes[0].inner_class, "Outer$Inner");
        assert_utf8_eq(classes[0].outer_class.expect("an enclosing class"), "Outer");
        assert_utf8_eq(classes[0].inner_name.expect("a simple name"), "Inner");
        assert_eq!(
            classes[0].access_flags,
            InnerClassAccessFlags::PRIVATE | InnerClassAccessFlags::STATIC
        );
        assert_utf8_eq(classes[1].inner_class, "Outer$1");
        assert!(classes[1].outer_class.is_none());
        assert!(classes[1].inner_name.is_none());
        assert_eq!(
            classes[1].access_flags,
            InnerClassAccessFlags::FINAL | InnerClassAccessFlags::SYNTHETIC
        );
    });
}

#[test]
fn inner_class_info_fields() {
    // Each of the four u2s is read in order and none is dropped.
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("InnerClasses");
        let inner = p.class("Test$E");
        let outer = p.class("Test");
        let simple = p.utf8("E");
        let body = a.attr_begin(name);
        a.u2(1);
        a.u2(inner);
        a.u2(outer);
        a.u2(simple);
        a.u2(0x4019); // PUBLIC | STATIC | FINAL | ENUM
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let classes =
            expect_attr!(only(&cf.attributes), AttributeInfo::InnerClasses { value } => value);
        assert_utf8_eq(classes[0].inner_class, "Test$E");
        assert_utf8_eq(classes[0].outer_class.unwrap(), "Test");
        assert_utf8_eq(classes[0].inner_name.unwrap(), "E");
        assert_eq!(classes[0].access_flags.bits(), 0x4019);
    });
}

#[test]
fn inner_classes_inner_class_index_must_be_class() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("InnerClasses");
        let not_class = p.utf8("Test$Inner");
        let outer = p.class("Test");
        let simple = p.utf8("Inner");
        let body = a.attr_begin(name);
        a.u2(1);
        a.u2(not_class);
        a.u2(outer);
        a.u2(simple);
        a.u2(0);
        a.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn enclosing_method_attribute() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("EnclosingMethod");
        let outer = p.class("Outer");
        let body = a.attr_begin(name);
        a.u2(outer);
        a.u2(0); // not enclosed by a method
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let (class, method) = expect_attr!(
            only(&cf.attributes),
            AttributeInfo::EnclosingMethod { enclosing_class, enclosing_method } =>
                (*enclosing_class, *enclosing_method)
        );
        assert_utf8_eq(class, "Outer");
        assert!(method.is_none());
    });
}

#[test]
fn enclosing_method_with_a_method() {
    // A class declared inside a method names it with a NameAndType.
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("EnclosingMethod");
        let outer = p.class("Outer");
        let nat = p.name_and_type("run", "()V");
        let body = a.attr_begin(name);
        a.u2(outer);
        a.u2(nat);
        a.attr_end(body);
        // The parser keeps the NameAndType index; the caller resolves it.
        assert!(nat > 0);
    });
    assert_parses(&bytes, |cf| {
        let method = expect_attr!(
            only(&cf.attributes),
            AttributeInfo::EnclosingMethod { enclosing_method, .. } => *enclosing_method
        );
        let index = method.expect("an enclosing method");
        assert!(matches!(
            cf.constant(index),
            Some(bytecode_vm::parser::class_file::ConstantPoolEntry::NameAndType { .. })
        ));
    });
}

#[test]
fn enclosing_method_class_index_must_be_class() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("EnclosingMethod");
        let not_class = p.utf8("Outer");
        let body = a.attr_begin(name);
        a.u2(not_class);
        a.u2(0);
        a.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

// ---------------------------------------------------------------------------
// Synthetic, Deprecated, Signature, SourceFile, SourceDebugExtension
// (4.7.8 - 4.7.11)
// ---------------------------------------------------------------------------

#[test]
fn synthetic_attribute() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("Synthetic");
        a.attr_empty(name);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::Synthetic => ())
    });
}

#[test]
fn deprecated_attribute() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("Deprecated");
        a.attr_empty(name);
    });
    assert_parses(&bytes, |cf| {
        expect_attr!(only(&cf.attributes), AttributeInfo::Deprecated => ())
    });
}

#[test]
fn signature_attribute() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("Signature");
        let sig = p.utf8("<T:Ljava/lang/Object;>Ljava/lang/Object;");
        a.attr_u2(name, sig);
    });
    assert_parses(&bytes, |cf| {
        let sig = expect_attr!(only(&cf.attributes), AttributeInfo::Signature { value } => *value);
        assert_utf8_eq(sig, "<T:Ljava/lang/Object;>Ljava/lang/Object;");
    });
}

#[test]
fn source_file_attribute() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("SourceFile");
        let file = p.utf8("Test.java");
        a.attr_u2(name, file);
    });
    assert_parses(&bytes, |cf| {
        let file =
            expect_attr!(only(&cf.attributes), AttributeInfo::SourceFileIndex { value } => *value);
        assert_utf8_eq(file, "Test.java");
    });
}

#[test]
fn source_file_index_must_be_utf8() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("SourceFile");
        let cls = p.class("Test");
        a.attr_u2(name, cls);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn source_debug_extension_attribute() {
    // The body is a free-form SMAP; it is not a constant pool index and is not
    // required to be valid modified UTF-8 either.
    const SMAP: &[u8] = b"SMAP\nTest.kt\nKotlin\n*S Kotlin\n*F\n+ 1 Test.kt\n*L\n1#1,10:1\n*E\n";
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("SourceDebugExtension");
        a.attr_raw(name, SMAP);
    });
    assert_parses(&bytes, |cf| {
        let value = expect_attr!(
            only(&cf.attributes),
            AttributeInfo::SourceDebugExtension { value } => *value
        );
        assert_eq!(value, SMAP);
    });
}

#[test]
fn source_debug_extension_empty() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("SourceDebugExtension");
        a.attr_raw(name, b"");
    });
    assert_parses(&bytes, |cf| {
        let value = expect_attr!(
            only(&cf.attributes),
            AttributeInfo::SourceDebugExtension { value } => *value
        );
        assert!(value.is_empty());
    });
}

#[test]
fn source_debug_extension_truncated_rejected() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("SourceDebugExtension");
        a.u2(name);
        a.u4(50);
        a.raw(b"SMAP");
    });
    assert_rejected_with!(&bytes, ClassParserError::Truncated(_));
}

// ---------------------------------------------------------------------------
// LineNumberTable, LocalVariableTable, LocalVariableTypeTable (4.7.12 - 4.7.14)
// ---------------------------------------------------------------------------

#[test]
fn line_number_table_attribute() {
    let bytes = code_with_attrs(1, |m, p| {
        let name = p.utf8("LineNumberTable");
        let body = m.attr_begin(name);
        m.u2(3);
        m.u2(0);
        m.u2(1);
        m.u2(5);
        m.u2(65535);
        m.u2(9);
        m.u2(1); // line numbers need not be monotonic
        m.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let code = find_attr!(cf.methods[0].attributes, AttributeInfo::Code { attributes, .. } => attributes);
        let entries = expect_attr!(only(code), AttributeInfo::LineNumberTable { entries } => entries);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].start_pc, 0);
        assert_eq!(entries[0].line_number, 1);
        assert_eq!(entries[1].start_pc, 5);
        assert_eq!(entries[1].line_number, 65535);
        assert_eq!(entries[2].start_pc, 9);
        assert_eq!(entries[2].line_number, 1);
    });
}

#[test]
fn multiple_line_number_tables_allowed() {
    // JVMS 4.7.12 lets the tables be split across several attributes.
    let bytes = code_with_attrs(2, |m, p| {
        let name = p.utf8("LineNumberTable");
        for (pc, line) in [(0u16, 1u16), (8, 2)] {
            let body = m.attr_begin(name);
            m.u2(1);
            m.u2(pc);
            m.u2(line);
            m.attr_end(body);
        }
    });
    assert_parses(&bytes, |cf| {
        let code = find_attr!(cf.methods[0].attributes, AttributeInfo::Code { attributes, .. } => attributes);
        let tables = code
            .iter()
            .filter(|a| matches!(a, AttributeInfo::LineNumberTable { .. }))
            .count();
        assert_eq!(tables, 2);
    });
}

#[test]
fn local_variable_table_attribute() {
    let bytes = code_with_attrs(1, |m, p| {
        let name = p.utf8("LocalVariableTable");
        let n1 = p.utf8("self");
        let d1 = p.utf8("LTest;");
        let n2 = p.utf8("count");
        let d2 = p.utf8("J");
        let body = m.attr_begin(name);
        m.u2(2);
        m.u2(0);
        m.u2(16);
        m.u2(n1);
        m.u2(d1);
        m.u2(0);
        m.u2(4);
        m.u2(12);
        m.u2(n2);
        m.u2(d2);
        m.u2(1); // a long occupies slots 1 and 2
        m.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let code = find_attr!(cf.methods[0].attributes, AttributeInfo::Code { attributes, .. } => attributes);
        let entries =
            expect_attr!(only(code), AttributeInfo::LocalVariableTable { entries } => entries);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].start_pc, 4);
        assert_eq!(entries[1].length, 12);
        assert_utf8_eq(entries[1].name, "count");
        assert_utf8_eq(entries[1].descriptor, "J");
        assert_eq!(entries[1].slot, 1);
    });
}

#[test]
fn local_variable_type_table_attribute() {
    // Same shape as LocalVariableTable, but the third index is a generic
    // signature rather than a descriptor (JVMS 4.7.14).
    let bytes = code_with_attrs(1, |m, p| {
        let name = p.utf8("LocalVariableTypeTable");
        let n = p.utf8("list");
        let s = p.utf8("Ljava/util/List<Ljava/lang/String;>;");
        let body = m.attr_begin(name);
        m.u2(1);
        m.u2(2);
        m.u2(10);
        m.u2(n);
        m.u2(s);
        m.u2(3);
        m.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let code = find_attr!(cf.methods[0].attributes, AttributeInfo::Code { attributes, .. } => attributes);
        let entries =
            expect_attr!(only(code), AttributeInfo::LocalVariableTypeTable { entries } => entries);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].start_pc, 2);
        assert_eq!(entries[0].length, 10);
        assert_utf8_eq(entries[0].name, "list");
        assert_utf8_eq(entries[0].signature, "Ljava/util/List<Ljava/lang/String;>;");
        assert_eq!(entries[0].slot, 3);
    });
}

#[test]
fn line_number_table_overruns_enclosing_code_rejected() {
    // A nested attribute is bounded by its parent too: this one claims ten
    // bytes where Code has only six left to give.
    let bytes = code_with_attrs(1, |m, p| {
        let name = p.utf8("LineNumberTable");
        m.u2(name);
        m.u4(10);
        m.u2(2);
        m.u2(0);
        m.u2(1);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn line_number_table_length_too_long_rejected() {
    // One entry read, four bytes still unread: a nested table has to fill its
    // own length exactly, just as a top-level attribute does.
    let bytes = code_with_attrs(1, |m, p| {
        let name = p.utf8("LineNumberTable");
        let body = m.attr_begin(name);
        m.u2(1);
        m.u2(0);
        m.u2(1);
        m.fill(0, 4);
        m.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn line_number_table_length_too_short_rejected() {
    // The other direction: two entries are promised and written, but the
    // declared length stops after the first.
    let bytes = code_with_attrs(1, |m, p| {
        let name = p.utf8("LineNumberTable");
        let body = m.attr_begin(name);
        m.u2(2);
        m.u2(0);
        m.u2(1);
        m.u2(4);
        m.u2(2);
        m.attr_end(body);
        m.patch_u4(body - 4, 6); // the real body is 10 bytes
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

// ---------------------------------------------------------------------------
// BootstrapMethods (4.7.23)
// ---------------------------------------------------------------------------

#[test]
fn bootstrap_methods_attribute() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("BootstrapMethods");
        let handle = p.bootstrap_handle();
        let arg0 = p.integer(1);
        let arg1 = p.string("s");
        let arg2 = p.class("java/lang/String");
        let body = a.attr_begin(name);
        a.u2(2);
        a.u2(handle);
        a.u2(0);
        a.u2(handle);
        a.u2(3);
        a.u2(arg0);
        a.u2(arg1);
        a.u2(arg2);
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let methods =
            expect_attr!(only(&cf.attributes), AttributeInfo::BootstrapMethods { value } => value);
        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].bootstrap_arguments.len(), 0);
        assert_eq!(methods[1].bootstrap_method_ref, methods[0].bootstrap_method_ref);
        assert_eq!(methods[1].bootstrap_arguments.len(), 3);
        // Arguments stay as indices: they may be of several constant kinds.
        for index in &methods[1].bootstrap_arguments {
            assert!(cf.constant(*index).is_some());
        }
    });
}

#[test]
fn bootstrap_methods_arguments_overrun_length_rejected() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("BootstrapMethods");
        let handle = p.bootstrap_handle();
        let body = a.attr_begin(name);
        a.u2(1);
        a.u2(handle);
        a.u2(5); // five arguments, two provided
        a.u2(1);
        a.u2(2);
        a.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn bootstrap_method_ref_must_be_method_handle() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("BootstrapMethods");
        let not_handle = p.methodref("Test", "bsm", "()V");
        let body = a.attr_begin(name);
        a.u2(1);
        a.u2(not_handle);
        a.u2(0);
        a.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn bootstrap_argument_must_be_loadable_constant() {
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("BootstrapMethods");
        let handle = p.bootstrap_handle();
        let nat = p.name_and_type("x", "I"); // NameAndType is not loadable
        let body = a.attr_begin(name);
        a.u2(1);
        a.u2(handle);
        a.u2(1);
        a.u2(nat);
        a.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn bootstrap_methods_with_loadable_arguments_accepted() {
    // Every constant kind JVMS 4.4 calls loadable is allowed as an argument.
    let bytes = class_with_attr(|a, p| {
        let name = p.utf8("BootstrapMethods");
        let handle = p.bootstrap_handle();
        let args = [
            p.integer(1),
            p.float_bits(0),
            p.long(2),
            p.double_bits(0),
            p.class("java/lang/String"),
            p.string("s"),
            p.method_type("()V"),
            handle,
        ];
        let body = a.attr_begin(name);
        a.u2(1);
        a.u2(handle);
        a.u2(args.len() as u16);
        for arg in args {
            a.u2(arg);
        }
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let methods =
            expect_attr!(only(&cf.attributes), AttributeInfo::BootstrapMethods { value } => value);
        assert_eq!(methods[0].bootstrap_arguments.len(), 8);
    });
}

#[test]
fn two_bootstrap_methods_attributes_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let handle = cb.pool.bootstrap_handle();
    cb.reserve_attributes(2);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, handle, 1);
    ClassBuilder::write_bootstrap_methods(&mut cb.attributes, &mut cb.pool, handle, 1);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

// ---------------------------------------------------------------------------
// MethodParameters (4.7.24)
// ---------------------------------------------------------------------------

#[test]
fn method_parameters_attribute() {
    let bytes = native_method_with_attrs_desc("(ILjava/lang/String;J)V", 1, |m, p| {
        let name = p.utf8("MethodParameters");
        let p1 = p.utf8("count");
        let body = m.attr_begin(name);
        m.u1(3); // parameters_count is a u1, unlike every other table count
        m.u2(p1);
        m.u2(acc::FINAL);
        m.u2(0); // unnamed
        m.u2(acc::SYNTHETIC);
        m.u2(p1);
        m.u2(acc::MANDATED);
        m.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let params = expect_attr!(
            only(&cf.methods[0].attributes),
            AttributeInfo::MethodParameters { value } => value
        );
        assert_eq!(params.len(), 3);
        assert_utf8_eq(params[0].name.expect("a named parameter"), "count");
        assert_eq!(params[0].access_flags, MethodParameterAccessFlags::FINAL);
        assert!(params[1].name.is_none(), "name_index 0 means unnamed");
        assert_eq!(params[1].access_flags, MethodParameterAccessFlags::SYNTHETIC);
        assert_eq!(params[2].access_flags, MethodParameterAccessFlags::MANDATED);
    });
}

#[test]
fn method_parameters_count_255() {
    // 255 is the most a u1 count, and a method descriptor, can hold.
    let desc = format!("({})V", "I".repeat(255));
    let bytes = native_method_with_attrs_desc(&desc, 1, |m, p| {
        let name = p.utf8("MethodParameters");
        let body = m.attr_begin(name);
        m.u1(255);
        for i in 0..255u16 {
            m.u2(0);
            m.u2(i & acc::FINAL);
        }
        m.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let params = expect_attr!(
            only(&cf.methods[0].attributes),
            AttributeInfo::MethodParameters { value } => value
        );
        assert_eq!(params.len(), 255);
    });
}

// ---------------------------------------------------------------------------
// NestHost, NestMembers, PermittedSubclasses (4.7.28, 4.7.29, 4.7.31)
// ---------------------------------------------------------------------------

#[test]
fn nest_host_attribute() {
    let bytes = class_attrs(55, 1, |a, p| {
        let name = p.utf8("NestHost");
        let host = p.class("Outer");
        a.attr_u2(name, host);
    });
    assert_parses(&bytes, |cf| {
        let host =
            expect_attr!(only(&cf.attributes), AttributeInfo::NestHostClass { value } => *value);
        assert_utf8_eq(host, "Outer");
    });
}

#[test]
fn nest_members_attribute() {
    let bytes = class_attrs(55, 1, |a, p| {
        let name = p.utf8("NestMembers");
        let m1 = p.class("Test$A");
        let m2 = p.class("Test$B");
        a.attr_u2_table(name, &[m1, m2]);
    });
    assert_parses(&bytes, |cf| {
        let members =
            expect_attr!(only(&cf.attributes), AttributeInfo::NestMembers { value } => value);
        assert_eq!(members.len(), 2);
        assert_utf8_eq(members[0], "Test$A");
        assert_utf8_eq(members[1], "Test$B");
    });
}

#[test]
fn nest_host_must_be_class() {
    let bytes = class_attrs(55, 1, |a, p| {
        let name = p.utf8("NestHost");
        let not_class = p.utf8("Outer");
        a.attr_u2(name, not_class);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn nest_host_and_nest_members_together_rejected() {
    // A class is either a nest host or a nest member, never both (JVMS 4.7.29).
    let bytes = class_attrs(55, 2, |a, p| {
        let host_name = p.utf8("NestHost");
        let host = p.class("Outer");
        let members_name = p.utf8("NestMembers");
        let member = p.class("Test$Inner");
        a.attr_u2(host_name, host);
        a.attr_u2_table(members_name, &[member]);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn two_nest_members_attributes_rejected() {
    let bytes = class_attrs(55, 2, |a, p| {
        let name = p.utf8("NestMembers");
        let member = p.class("Test$Inner");
        a.attr_u2_table(name, &[member]);
        a.attr_u2_table(name, &[member]);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn permitted_subclasses_attribute() {
    let bytes = class_attrs(61, 1, |a, p| {
        let name = p.utf8("PermittedSubclasses");
        let c1 = p.class("Circle");
        let c2 = p.class("Square");
        let c3 = p.class("Triangle");
        a.attr_u2_table(name, &[c1, c2, c3]);
    });
    assert_parses(&bytes, |cf| {
        let subs =
            expect_attr!(only(&cf.attributes), AttributeInfo::PermittedSubclasses { value } => value);
        assert_eq!(subs.len(), 3);
        assert_utf8_eq(subs[2], "Triangle");
    });
}

#[test]
fn permitted_subclasses_in_final_class_rejected() {
    // A final class cannot be sealed: nothing may extend it (JVMS 4.7.31).
    let mut cb = ClassBuilder::new(61);
    cb.access_flags = acc::PUBLIC | acc::SUPER | acc::FINAL;
    let name = cb.pool.utf8("PermittedSubclasses");
    let sub = cb.pool.class("Sub");
    cb.reserve_attributes(1);
    cb.attributes.attr_u2_table(name, &[sub]);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn two_permitted_subclasses_attributes_rejected() {
    let bytes = class_attrs(61, 2, |a, p| {
        let name = p.utf8("PermittedSubclasses");
        let sub = p.class("Sub");
        a.attr_u2_table(name, &[sub]);
        a.attr_u2_table(name, &[sub]);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

// ---------------------------------------------------------------------------
// Record (4.7.30)
// ---------------------------------------------------------------------------

#[test]
fn record_attribute() {
    let bytes = record_class(&[("x", "I"), ("items", "Ljava/util/List;")], |a, p| {
        let name = p.utf8("Record");
        let x = p.utf8("x");
        let xd = p.utf8("I");
        let items = p.utf8("items");
        let itemsd = p.utf8("Ljava/util/List;");
        let sig_name = p.utf8("Signature");
        let sig = p.utf8("Ljava/util/List<Ljava/lang/String;>;");
        let body = a.attr_begin(name);
        a.u2(2);
        a.u2(x);
        a.u2(xd);
        a.u2(0);
        // A component carries its own attribute table, here a generic signature.
        a.u2(items);
        a.u2(itemsd);
        a.u2(1);
        a.attr_u2(sig_name, sig);
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let record = expect_attr!(only(&cf.attributes), AttributeInfo::Record { value } => value);
        let components = &record.record_components;
        assert_eq!(components.len(), 2);
        assert_utf8_eq(components[0].name, "x");
        assert_utf8_eq(components[0].descriptor, "I");
        assert!(components[0].attributes.is_empty());
        assert_utf8_eq(components[1].name, "items");
        assert_utf8_eq(components[1].descriptor, "Ljava/util/List;");
        let sig = expect_attr!(
            only(&components[1].attributes),
            AttributeInfo::Signature { value } => *value
        );
        assert_utf8_eq(sig, "Ljava/util/List<Ljava/lang/String;>;");
    });
}

#[test]
fn record_attribute_empty() {
    // A record with no components is legal.
    let bytes = record_class(&[], |a, p| {
        let name = p.utf8("Record");
        let body = a.attr_begin(name);
        a.u2(0);
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let record = expect_attr!(only(&cf.attributes), AttributeInfo::Record { value } => value);
        assert!(record.record_components.is_empty());
    });
}

#[test]
fn record_component_unknown_attribute_skipped() {
    let bytes = record_class(&[("x", "I")], |a, p| {
        let name = p.utf8("Record");
        let x = p.utf8("x");
        let xd = p.utf8("I");
        let custom = p.utf8("Custom");
        let body = a.attr_begin(name);
        a.u2(1);
        a.u2(x);
        a.u2(xd);
        a.u2(1);
        a.attr_raw(custom, b"abc");
        a.attr_end(body);
    });
    assert_parses(&bytes, |cf| {
        let record = expect_attr!(only(&cf.attributes), AttributeInfo::Record { value } => value);
        expect_attr!(
            only(&record.record_components[0].attributes),
            AttributeInfo::Unknown { .. } => ()
        );
    });
}

#[test]
fn record_component_descriptor_must_be_field_descriptor() {
    // A component is a field, so "()V" cannot be its descriptor.
    let bytes = record_class(&[("x", "I")], |a, p| {
        let name = p.utf8("Record");
        let x = p.utf8("x");
        let bad = p.utf8("()V");
        let body = a.attr_begin(name);
        a.u2(1);
        a.u2(x);
        a.u2(bad);
        a.u2(0);
        a.attr_end(body);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

// ---------------------------------------------------------------------------
// "At most one" rules (JVMS 4.7, table 4.7-C)
// ---------------------------------------------------------------------------

/// Two copies of a u2-bodied class attribute, which is one too many.
fn parse_with_two_u2_attrs(name: &str, major: u16) -> Vec<u8> {
    class_attrs(major, 2, |a, p| {
        let idx = p.utf8(name);
        let value = match name {
            "SourceFile" | "Signature" => p.utf8("Ljava/lang/Object;"),
            _ => p.class("Other"),
        };
        a.attr_u2(idx, value);
        a.attr_u2(idx, value);
    })
}

#[test]
fn two_source_file_attributes_rejected() {
    assert_rejected_with!(
        &parse_with_two_u2_attrs("SourceFile", 52),
        ClassParserError::ClassParseFormatError
    );
}

#[test]
fn two_signature_attributes_rejected() {
    assert_rejected_with!(
        &parse_with_two_u2_attrs("Signature", 52),
        ClassParserError::ClassParseFormatError
    );
}

#[test]
fn two_nest_host_attributes_rejected() {
    assert_rejected_with!(
        &parse_with_two_u2_attrs("NestHost", 55),
        ClassParserError::ClassParseFormatError
    );
}

#[test]
fn two_inner_classes_attributes_rejected() {
    let bytes = class_attrs(DEFAULT_MAJOR_VERSION, 2, |a, p| {
        let name = p.utf8("InnerClasses");
        for _ in 0..2 {
            let body = a.attr_begin(name);
            a.u2(0);
            a.attr_end(body);
        }
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn two_enclosing_method_attributes_rejected() {
    let bytes = class_attrs(DEFAULT_MAJOR_VERSION, 2, |a, p| {
        let name = p.utf8("EnclosingMethod");
        let outer = p.class("Outer");
        for _ in 0..2 {
            let body = a.attr_begin(name);
            a.u2(outer);
            a.u2(0);
            a.attr_end(body);
        }
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn two_source_debug_extension_attributes_rejected() {
    let bytes = class_attrs(DEFAULT_MAJOR_VERSION, 2, |a, p| {
        let name = p.utf8("SourceDebugExtension");
        a.attr_raw(name, b"SMAP");
        a.attr_raw(name, b"SMAP");
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn two_constant_value_attributes_on_field_rejected() {
    let bytes = field_with_attrs(2, |f, p| {
        let cv = p.utf8("ConstantValue");
        let v = p.integer(1);
        f.attr_u2(cv, v);
        f.attr_u2(cv, v);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn two_code_attributes_on_method_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC | acc::STATIC, "m", "()V", 2);
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 0, 0, &[op::RETURN]);
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 0, 0, &[op::RETURN]);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn two_exceptions_attributes_on_method_rejected() {
    let bytes = native_method_with_attrs(2, |m, p| {
        let name = p.utf8("Exceptions");
        let c = p.class("java/io/IOException");
        m.attr_u2_table(name, &[c]);
        m.attr_u2_table(name, &[c]);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn class_with_many_attribute_kinds() {
    // The common case: several unrelated attributes side by side, all kept.
    let bytes = class_attrs(61, 7, |a, p| {
        let file_name = p.utf8("SourceFile");
        let file = p.utf8("Test.java");
        let sig_name = p.utf8("Signature");
        let sig = p.utf8("<T:Ljava/lang/Object;>Ljava/lang/Object;");
        let deprecated = p.utf8("Deprecated");
        let synthetic = p.utf8("Synthetic");
        let members_name = p.utf8("NestMembers");
        let member = p.class("Test$Inner");
        let subs_name = p.utf8("PermittedSubclasses");
        let sde_name = p.utf8("SourceDebugExtension");

        a.attr_u2(file_name, file);
        a.attr_u2(sig_name, sig);
        a.attr_empty(deprecated);
        a.attr_empty(synthetic);
        a.attr_u2_table(members_name, &[member]);
        a.attr_u2_table(subs_name, &[member]);
        a.attr_raw(sde_name, b"SMAP");
    });
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.attributes.len(), 7);
        let file =
            find_attr!(cf.attributes, AttributeInfo::SourceFileIndex { value } => *value);
        assert_utf8_eq(file, "Test.java");
        let sig = find_attr!(cf.attributes, AttributeInfo::Signature { value } => *value);
        assert_utf8_eq(sig, "<T:Ljava/lang/Object;>Ljava/lang/Object;");
        find_attr!(cf.attributes, AttributeInfo::Deprecated => ());
        find_attr!(cf.attributes, AttributeInfo::Synthetic => ());
        find_attr!(cf.attributes, AttributeInfo::NestMembers { .. } => ());
        find_attr!(cf.attributes, AttributeInfo::PermittedSubclasses { .. } => ());
        let sde =
            find_attr!(cf.attributes, AttributeInfo::SourceDebugExtension { value } => *value);
        assert_eq!(sde.len(), 4);
        let unknown = cf
            .attributes
            .iter()
            .filter(|a| matches!(a, AttributeInfo::Unknown { .. }))
            .count();
        assert_eq!(unknown, 0, "every attribute here is a known one");
    });
}
