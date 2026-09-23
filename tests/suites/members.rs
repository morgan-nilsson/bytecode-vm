//! Fields (JVMS 4.5) and methods (JVMS 4.6): the shape a `field_info` and a
//! `method_info` parse into, then the access flag, name, descriptor and
//! attribute rules the parser has to enforce.
//!
//! Ported from `c_backup/tests/test_classfile_members.c`. The C suite could
//! call `field_info_read` / `method_info_read` on a bare buffer; here every
//! case goes through `ClassFile::parse` and asserts on `cf.fields` /
//! `cf.methods`, because a member's validity depends on the class around it
//! (its version, its ACC_INTERFACE bit, its constant pool) anyway.


use crate::common::*;

use bytecode_vm::parser::class_file::{
    AttributeInfo, ClassParserError, ConstantPoolEntry, FieldInfoAccessFlags, MethodInfoAccessFlags,
};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// `public class Test extends Object`.
const CLASS_FLAGS: u16 = acc::PUBLIC | acc::SUPER;
/// `public interface Test`.
const INTERFACE_FLAGS: u16 = acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT;
/// `public abstract class Test extends Object`, needed to hold abstract methods.
const ABSTRACT_CLASS_FLAGS: u16 = acc::PUBLIC | acc::SUPER | acc::ABSTRACT;

/// A whole class whose only member is one attribute-less field.
fn field_class(class_flags: u16, field_flags: u16, name: &str, desc: &str) -> Vec<u8> {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = class_flags;
    cb.add_field(field_flags, name, desc, 0);
    cb.to_bytes()
}

/// A whole class whose only member is one method. Abstract and native methods
/// must not carry Code (JVMS 4.7.3), so those are written without it and every
/// other method gets a trivial body.
fn method_class(
    major: u16,
    class_flags: u16,
    method_flags: u16,
    name: &str,
    desc: &str,
) -> Vec<u8> {
    let mut cb = ClassBuilder::new(major);
    cb.access_flags = class_flags;
    if method_flags & (acc::ABSTRACT | acc::NATIVE) != 0 {
        cb.add_method(method_flags, name, desc, 0);
    } else {
        cb.add_simple_method(method_flags, name, desc);
    }
    cb.to_bytes()
}

// ---------------------------------------------------------------------------
// field_info / method_info
// ---------------------------------------------------------------------------

#[test]
fn field_info_read_without_attributes() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_field(acc::PRIVATE | acc::VOLATILE, "count", "I", 0);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        assert_eq!(cf.fields.len(), 1);
        let f = &cf.fields[0];
        assert_eq!(
            f.access_flags,
            FieldInfoAccessFlags::PRIVATE | FieldInfoAccessFlags::VOLATILE
        );
        assert_utf8_eq(f.name, "count");
        assert_utf8_eq(f.descriptor, "I");
        assert!(f.attributes.is_empty());
    });
}

#[test]
fn field_info_read_with_constant_value() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let cv = cb.pool.utf8("ConstantValue");
    let value = cb.pool.long(1234567890123);
    cb.add_field(acc::PUBLIC | acc::STATIC | acc::FINAL, "MAX", "J", 1);
    cb.fields.attr_u2(cv, value);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        let f = &cf.fields[0];
        assert_eq!(f.attributes.len(), 1);
        // The attribute holds a pool index, so the long lives in the pool.
        let AttributeInfo::ConstantValueIndex { value: index } = f.attributes[0] else {
            panic!("expected a ConstantValue attribute");
        };
        assert_eq!(index, value);
        assert!(matches!(cf.constant(index), Some(ConstantPoolEntry::Long(1234567890123))));
    });
}

#[test]
fn field_info_read_with_several_attributes() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let sig_name = cb.pool.utf8("Signature");
    let sig = cb.pool.utf8("Ljava/util/List<Ljava/lang/String;>;");
    let synthetic = cb.pool.utf8("Synthetic");
    let deprecated = cb.pool.utf8("Deprecated");
    cb.add_field(acc::PROTECTED, "list", "Ljava/util/List;", 3);
    cb.fields.attr_u2(sig_name, sig);
    cb.fields.attr_empty(synthetic);
    cb.fields.attr_empty(deprecated);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        let f = &cf.fields[0];
        assert_eq!(f.attributes.len(), 3);
        // Attribute order is the file's order, not a canonical one.
        let AttributeInfo::Signature { value } = f.attributes[0] else {
            panic!("expected Signature first");
        };
        assert_utf8_eq(value, "Ljava/util/List<Ljava/lang/String;>;");
        assert!(matches!(f.attributes[1], AttributeInfo::Synthetic));
        assert!(matches!(f.attributes[2], AttributeInfo::Deprecated));
    });
}

#[test]
fn field_info_read_skips_unknown_attribute() {
    // JVMS 4.7.1: an attribute the parser doesn't know must be skipped by its
    // declared length, leaving the next attribute readable.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let vendor = cb.pool.utf8("com.example.VendorData");
    let sig_name = cb.pool.utf8("Signature");
    let sig = cb.pool.utf8("I");
    cb.add_field(0, "x", "I", 2);
    cb.fields.attr_raw(vendor, &[0x01, 0x02, 0x03, 0x04, 0x05]);
    cb.fields.attr_u2(sig_name, sig);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        let f = &cf.fields[0];
        assert_eq!(f.attributes.len(), 2);
        let AttributeInfo::Unknown { name, info } = f.attributes[0] else {
            panic!("expected the vendor attribute to be kept as Unknown");
        };
        assert_utf8_eq(name, "com.example.VendorData");
        assert_eq!(info, &[0x01, 0x02, 0x03, 0x04, 0x05]);
        assert!(matches!(f.attributes[1], AttributeInfo::Signature { .. }));
    });
}

#[test]
fn field_info_read_truncated_rejected() {
    // Every short read has to surface as an error rather than a half-built
    // field; cutting the file at each byte is the cheap way to prove it.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let sig_name = cb.pool.utf8("Signature");
    let sig = cb.pool.utf8("I");
    cb.add_field(0, "x", "I", 1);
    cb.fields.attr_u2(sig_name, sig);
    let bytes = cb.to_bytes();

    for len in 0..bytes.len() {
        assert_rejected_with!(&bytes[..len], ClassParserError::Truncated(_));
    }
}

#[test]
fn method_info_read_with_code() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC | acc::STATIC, "answer", "()I", 1);
    let body = [0x10, 0x2a, op::IRETURN]; // bipush 42; ireturn
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 1, 0, &body);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        assert_eq!(cf.methods.len(), 1);
        let m = &cf.methods[0];
        assert_eq!(
            m.access_flags,
            MethodInfoAccessFlags::PUBLIC | MethodInfoAccessFlags::STATIC
        );
        assert_utf8_eq(m.name, "answer");
        assert_utf8_eq(m.descriptor, "()I");
        assert_eq!(m.attributes.len(), 1);
        let AttributeInfo::Code { max_stack, code, .. } = &m.attributes[0] else {
            panic!("expected a Code attribute");
        };
        assert_eq!(*max_stack, 1);
        // The code array borrows the file's bytes verbatim.
        assert_eq!(*code, &body);
    });
}

#[test]
fn method_info_read_abstract_without_attributes() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "run", "()V", 0);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        let m = &cf.methods[0];
        assert_eq!(
            m.access_flags,
            MethodInfoAccessFlags::PUBLIC | MethodInfoAccessFlags::ABSTRACT
        );
        assert!(m.attributes.is_empty());
    });
}

#[test]
fn method_info_read_with_exceptions_and_signature() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    let exc_name = cb.pool.utf8("Exceptions");
    let sig_name = cb.pool.utf8("Signature");
    let sig = cb.pool.utf8("()TV;");
    let io = cb.pool.class("java/io/IOException");
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "call", "()Ljava/lang/Object;", 2);
    cb.methods.attr_u2_table(exc_name, &[io]);
    cb.methods.attr_u2(sig_name, sig);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        let m = &cf.methods[0];
        assert_eq!(m.attributes.len(), 2);
        let AttributeInfo::Exceptions { entries } = &m.attributes[0] else {
            panic!("expected an Exceptions attribute");
        };
        assert_eq!(entries.len(), 1);
        assert_utf8_eq(entries[0], "java/io/IOException");
        assert!(matches!(m.attributes[1], AttributeInfo::Signature { .. }));
    });
}

#[test]
fn method_info_read_attribute_error_propagates() {
    // A failure inside an attribute must abort the whole parse, not be
    // swallowed as "unknown attribute".
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", "()V", 1);
    cb.methods.attr_raw(200, &[]); // attribute_name_index past the end of the pool
    let bytes = cb.to_bytes();

    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

// ---------------------------------------------------------------------------
// Access flag bits
// ---------------------------------------------------------------------------

#[test]
fn field_access_flag_macros() {
    // Each bit in FieldInfoAccessFlags has to map to the JVMS value, and no
    // neighbouring bit may set it by accident.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_field(acc::PRIVATE | acc::VOLATILE, "a", "I", 0);
    cb.add_field(
        acc::PUBLIC | acc::STATIC | acc::FINAL | acc::TRANSIENT | acc::SYNTHETIC | acc::ENUM,
        "b",
        "I",
        0,
    );
    cb.add_field(acc::PROTECTED, "c", "I", 0);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        let (a, b, c) = (&cf.fields[0], &cf.fields[1], &cf.fields[2]);
        assert!(a.access_flags.contains(FieldInfoAccessFlags::PRIVATE));
        assert!(a.access_flags.contains(FieldInfoAccessFlags::VOLATILE));
        assert!(!a.access_flags.contains(FieldInfoAccessFlags::PUBLIC));
        assert!(!a.access_flags.contains(FieldInfoAccessFlags::TRANSIENT));

        assert!(b.access_flags.contains(FieldInfoAccessFlags::PUBLIC));
        assert!(b.access_flags.contains(FieldInfoAccessFlags::STATIC));
        assert!(b.access_flags.contains(FieldInfoAccessFlags::FINAL));
        assert!(b.access_flags.contains(FieldInfoAccessFlags::TRANSIENT));
        assert!(b.access_flags.contains(FieldInfoAccessFlags::SYNTHETIC));
        assert!(b.access_flags.contains(FieldInfoAccessFlags::ENUM));
        assert!(!b.access_flags.contains(FieldInfoAccessFlags::VOLATILE));

        assert!(c.access_flags.contains(FieldInfoAccessFlags::PROTECTED));
        assert!(!c.access_flags.contains(FieldInfoAccessFlags::PRIVATE));
    });
}

#[test]
fn method_access_flag_macros() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_simple_method(
        acc::PUBLIC
            | acc::STATIC
            | acc::FINAL
            | acc::SYNCHRONIZED
            | acc::BRIDGE
            | acc::VARARGS
            | acc::STRICT
            | acc::SYNTHETIC,
        "a",
        "()V",
    );
    cb.add_method(acc::PROTECTED | acc::ABSTRACT, "b", "()V", 0);
    cb.add_method(acc::PRIVATE | acc::NATIVE, "c", "()V", 0);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        let (a, b, c) = (&cf.methods[0], &cf.methods[1], &cf.methods[2]);
        for flag in [
            MethodInfoAccessFlags::PUBLIC,
            MethodInfoAccessFlags::STATIC,
            MethodInfoAccessFlags::FINAL,
            MethodInfoAccessFlags::SYNCHRONIZED,
            MethodInfoAccessFlags::BRIDGE,
            MethodInfoAccessFlags::VARARGS,
            MethodInfoAccessFlags::STRICT,
            MethodInfoAccessFlags::SYNTHETIC,
        ] {
            assert!(a.access_flags.contains(flag), "method a lost {flag:?}");
        }
        assert!(!a.access_flags.contains(MethodInfoAccessFlags::NATIVE));
        assert!(!a.access_flags.contains(MethodInfoAccessFlags::ABSTRACT));

        assert!(b.access_flags.contains(MethodInfoAccessFlags::PROTECTED));
        assert!(b.access_flags.contains(MethodInfoAccessFlags::ABSTRACT));

        assert!(c.access_flags.contains(MethodInfoAccessFlags::PRIVATE));
        assert!(c.access_flags.contains(MethodInfoAccessFlags::NATIVE));
        assert!(!c.access_flags.contains(MethodInfoAccessFlags::SYNCHRONIZED));
    });
}

// ---------------------------------------------------------------------------
// Fields through ClassFile::parse
// ---------------------------------------------------------------------------

#[test]
fn fields_parsed_in_order() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_field(acc::PRIVATE, "a", "I", 0);
    cb.add_field(acc::PUBLIC | acc::STATIC, "b", "[Ljava/lang/String;", 0);
    cb.add_field(acc::PROTECTED | acc::TRANSIENT, "c", "D", 0);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        assert_eq!(cf.fields.len(), 3);
        assert_utf8_eq(cf.fields[0].name, "a");
        assert_utf8_eq(cf.fields[0].descriptor, "I");
        assert_eq!(cf.fields[0].access_flags, FieldInfoAccessFlags::PRIVATE);
        assert_utf8_eq(cf.fields[1].name, "b");
        assert_utf8_eq(cf.fields[1].descriptor, "[Ljava/lang/String;");
        assert_eq!(
            cf.fields[1].access_flags,
            FieldInfoAccessFlags::PUBLIC | FieldInfoAccessFlags::STATIC
        );
        assert_utf8_eq(cf.fields[2].name, "c");
        assert_eq!(
            cf.fields[2].access_flags,
            FieldInfoAccessFlags::PROTECTED | FieldInfoAccessFlags::TRANSIENT
        );
    });
}

#[test]
fn field_constant_values_of_each_type() {
    // JVMS 4.7.2 pairs each field descriptor with the one constant kind its
    // ConstantValue may be; all nine have to be accepted.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let cv = cb.pool.utf8("ConstantValue");
    let flags = acc::STATIC | acc::FINAL;

    let int_values: [(&str, &str, i32); 5] =
        [("I", "I", 1), ("S", "S", 2), ("C", "C", 'c' as i32), ("B", "B", -1), ("Z", "Z", 1)];
    for (name, desc, value) in int_values {
        let v = cb.pool.integer(value);
        cb.add_field(flags, name, desc, 1);
        cb.fields.attr_u2(cv, v);
    }
    let long_value = cb.pool.long(5);
    cb.add_field(flags, "J", "J", 1);
    cb.fields.attr_u2(cv, long_value);
    let float_value = cb.pool.float_bits(0x3F80_0000);
    cb.add_field(flags, "F", "F", 1);
    cb.fields.attr_u2(cv, float_value);
    let double_value = cb.pool.double_bits(0x3FF0_0000_0000_0000);
    cb.add_field(flags, "D", "D", 1);
    cb.fields.attr_u2(cv, double_value);
    let string_value = cb.pool.string("s");
    cb.add_field(flags, "Str", "Ljava/lang/String;", 1);
    cb.fields.attr_u2(cv, string_value);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        assert_eq!(cf.fields.len(), 9);
        for f in &cf.fields {
            assert_eq!(f.attributes.len(), 1);
            assert!(
                matches!(f.attributes[0], AttributeInfo::ConstantValueIndex { .. }),
                "field {:?} lost its ConstantValue",
                f.name
            );
        }
        let j = cf.fields.iter().find(|f| f.name.as_bytes() == b"J").expect("field J is missing");
        let AttributeInfo::ConstantValueIndex { value } = j.attributes[0] else {
            unreachable!()
        };
        assert!(matches!(cf.constant(value), Some(ConstantPoolEntry::Long(5))));
    });
}

#[test]
fn field_constant_value_type_mismatch_rejected() {
    // An int field's ConstantValue must be a CONSTANT_Integer.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let cv = cb.pool.utf8("ConstantValue");
    let value = cb.pool.string("not an int");
    cb.add_field(acc::STATIC | acc::FINAL, "x", "I", 1);
    cb.fields.attr_u2(cv, value);

    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn field_constant_value_long_for_int_field_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let cv = cb.pool.utf8("ConstantValue");
    let value = cb.pool.long(1);
    cb.add_field(acc::STATIC | acc::FINAL, "x", "I", 1);
    cb.fields.attr_u2(cv, value);

    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn field_constant_value_index_out_of_range_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let cv = cb.pool.utf8("ConstantValue");
    cb.add_field(acc::STATIC | acc::FINAL, "x", "I", 1);
    cb.fields.attr_u2(cv, 900);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn field_two_constant_value_attributes_rejected() {
    // JVMS 4.7.2: at most one ConstantValue per field.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let cv = cb.pool.utf8("ConstantValue");
    let value = cb.pool.integer(1);
    cb.add_field(acc::STATIC | acc::FINAL, "x", "I", 2);
    cb.fields.attr_u2(cv, value);
    cb.fields.attr_u2(cv, value);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn field_name_index_not_utf8_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let this_class = cb.this_class;
    let desc = cb.pool.utf8("I");
    cb.fields.u2(acc::PRIVATE);
    cb.fields.u2(this_class); // a Class entry, not Utf8
    cb.fields.u2(desc);
    cb.fields.u2(0);
    cb.fields_count += 1;

    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn field_name_index_out_of_range_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let desc = cb.pool.utf8("I");
    cb.fields.u2(acc::PRIVATE);
    cb.fields.u2(4000);
    cb.fields.u2(desc);
    cb.fields.u2(0);
    cb.fields_count += 1;

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn field_descriptor_index_zero_rejected() {
    // Index 0 is never a valid constant pool reference (JVMS 4.4).
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let name = cb.pool.utf8("x");
    cb.fields.u2(acc::PRIVATE);
    cb.fields.u2(name);
    cb.fields.u2(0);
    cb.fields.u2(0);
    cb.fields_count += 1;

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn field_descriptor_index_unusable_long_slot_rejected() {
    // A Long occupies two slots and the second names no entry (JVMS 4.4.5),
    // so an index pointing at it is in range yet still unusable.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let name = cb.pool.utf8("x");
    let long = cb.pool.long(0);
    cb.fields.u2(acc::PRIVATE);
    cb.fields.u2(name);
    cb.fields.u2(long + 1);
    cb.fields.u2(0);
    cb.fields_count += 1;

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn field_valid_descriptors_accepted() {
    let descs = [
        "B",
        "C",
        "D",
        "F",
        "I",
        "J",
        "S",
        "Z",
        "Ljava/lang/Object;",
        "[I",
        "[[[Ljava/lang/String;",
        "La/b/c/D$E;",
    ];
    for desc in descs {
        let bytes = field_class(CLASS_FLAGS, acc::PRIVATE, "f", desc);
        assert_parses(&bytes, |cf| {
            assert_utf8_eq(cf.fields[0].descriptor, desc);
        });
    }
}

#[test]
fn field_invalid_descriptors_rejected() {
    let descs = [
        "",
        "V",                     // void is not a field type
        "()V",                   // a method descriptor
        "Q",                     // not a base type character
        "L;",                    // empty class name
        "Ljava/lang/Object",     // unterminated
        "[",                     // no component type
        "II",                    // trailing junk
        "[V",
        "java/lang/Object",      // missing the L ... ;
        "Ljava.lang.Object;",    // dots are not allowed in an internal name
        "Ljava/lang/Object;I",
    ];
    for desc in descs {
        let bytes = field_class(CLASS_FLAGS, acc::PRIVATE, "f", desc);
        assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
    }
}

#[test]
fn field_array_of_256_dimensions_rejected() {
    // A descriptor may not have more than 255 array dimensions (JVMS 4.3.2).
    let mut too_deep = "[".repeat(256);
    too_deep.push('I');
    assert_rejected_with!(
        &field_class(CLASS_FLAGS, acc::PRIVATE, "f", &too_deep),
        ClassParserError::ClassParseFormatError
    );

    let mut at_limit = "[".repeat(255);
    at_limit.push('I');
    assert_parses(&field_class(CLASS_FLAGS, acc::PRIVATE, "f", &at_limit), |cf| {
        assert_eq!(cf.fields.len(), 1);
    });
}

#[test]
fn field_invalid_names_rejected() {
    // Unqualified names may not contain . ; [ / (JVMS 4.2.2).
    for name in ["", "a.b", "a;b", "a[b", "a/b"] {
        assert_rejected_with!(
            &field_class(CLASS_FLAGS, acc::PRIVATE, name, "I"),
            ClassParserError::ClassParseFormatError
        );
    }
}

#[test]
fn field_unusual_but_legal_names_accepted() {
    // A field name only has to be an unqualified name: < and > are fine here
    // even though they are forbidden in method names, and so is any non-ASCII
    // character or a name that could not be written in Java source.
    for name in ["<init>", "$", "_", "a-b", "é", "123"] {
        assert_parses(&field_class(CLASS_FLAGS, acc::PRIVATE, name, "I"), |cf| {
            assert_utf8_eq(cf.fields[0].name, name);
        });
    }
}

#[test]
fn field_more_than_one_visibility_rejected() {
    assert_rejected_with!(
        &field_class(CLASS_FLAGS, acc::PUBLIC | acc::PRIVATE, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &field_class(CLASS_FLAGS, acc::PUBLIC | acc::PROTECTED, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &field_class(CLASS_FLAGS, acc::PRIVATE | acc::PROTECTED, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
}

#[test]
fn field_final_and_volatile_rejected() {
    assert_rejected_with!(
        &field_class(CLASS_FLAGS, acc::FINAL | acc::VOLATILE, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
}

#[test]
fn field_all_legal_class_flags_accepted() {
    let flags =
        acc::PUBLIC | acc::STATIC | acc::FINAL | acc::TRANSIENT | acc::SYNTHETIC | acc::ENUM;
    assert_parses(&field_class(CLASS_FLAGS, flags, "f", "I"), |cf| {
        assert_eq!(cf.fields.len(), 1);
    });
}

#[test]
fn field_reserved_flags_ignored() {
    // Bits with no meaning for a field must be dropped, not rejected, so that
    // a class file from a newer release still loads (JVMS 4.5).
    let flags = acc::PRIVATE | 0x0100 | 0x0800 | 0x8000;
    assert_parses(&field_class(CLASS_FLAGS, flags, "f", "I"), |cf| {
        assert_eq!(cf.fields[0].access_flags, FieldInfoAccessFlags::PRIVATE);
    });
}

#[test]
fn interface_field_public_static_final_accepted() {
    let flags = acc::PUBLIC | acc::STATIC | acc::FINAL;
    assert_parses(&field_class(INTERFACE_FLAGS, flags, "f", "I"), |cf| {
        assert_eq!(cf.fields.len(), 1);
    });
    assert_parses(&field_class(INTERFACE_FLAGS, flags | acc::SYNTHETIC, "f", "I"), |cf| {
        assert_eq!(cf.fields.len(), 1);
    });
}

#[test]
fn interface_field_not_public_static_final_rejected() {
    // Every interface field must have all three of them (JVMS 4.5).
    assert_rejected_with!(
        &field_class(INTERFACE_FLAGS, acc::STATIC | acc::FINAL, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &field_class(INTERFACE_FLAGS, acc::PUBLIC | acc::FINAL, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &field_class(INTERFACE_FLAGS, acc::PUBLIC | acc::STATIC, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &field_class(INTERFACE_FLAGS, acc::PRIVATE | acc::STATIC | acc::FINAL, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
}

#[test]
fn interface_field_volatile_or_transient_rejected() {
    let base = acc::PUBLIC | acc::STATIC | acc::FINAL;
    assert_rejected_with!(
        &field_class(INTERFACE_FLAGS, base | acc::TRANSIENT, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    // VOLATILE is equally forbidden, and here it does not even collide with FINAL.
    assert_rejected_with!(
        &field_class(INTERFACE_FLAGS, base | acc::VOLATILE, "f", "I"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
}

#[test]
fn duplicate_field_rejected() {
    // Name and descriptor together identify a field (JVMS 4.5); differing
    // access flags do not make them distinct.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_field(acc::PRIVATE, "dup", "I", 0);
    cb.add_field(acc::PUBLIC, "dup", "I", 0);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn same_field_name_different_descriptor_accepted() {
    // Legal in class files even though Java source can't express it.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_field(acc::PRIVATE, "x", "I", 0);
    cb.add_field(acc::PRIVATE, "x", "J", 0);

    assert_parses(&cb.to_bytes(), |cf| {
        assert_eq!(cf.fields.len(), 2);
    });
}

#[test]
fn fields_count_larger_than_data_rejected() {
    // Left unpinned: only four bytes follow the field, so whether the parser
    // runs off the end or first rejects the zero name_index it reads out of
    // the count that follows depends on the order it validates in.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_field(acc::PRIVATE, "x", "I", 0);
    cb.fields_count = 2;

    assert_rejected(&cb.to_bytes());
}

// ---------------------------------------------------------------------------
// Methods through ClassFile::parse
// ---------------------------------------------------------------------------

#[test]
fn methods_parsed_in_order() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_simple_method(acc::PUBLIC, "<init>", "()V");
    cb.add_simple_method(acc::PUBLIC | acc::STATIC, "main", "([Ljava/lang/String;)V");
    cb.add_simple_method(acc::PRIVATE | acc::SYNCHRONIZED, "count", "(IJD)I");
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        assert_eq!(cf.methods.len(), 3);
        assert_utf8_eq(cf.methods[0].name, "<init>");
        assert_utf8_eq(cf.methods[1].name, "main");
        assert_utf8_eq(cf.methods[1].descriptor, "([Ljava/lang/String;)V");
        assert_eq!(
            cf.methods[1].access_flags,
            MethodInfoAccessFlags::PUBLIC | MethodInfoAccessFlags::STATIC
        );
        assert_utf8_eq(cf.methods[2].descriptor, "(IJD)I");
        assert_eq!(
            cf.methods[2].access_flags,
            MethodInfoAccessFlags::PRIVATE | MethodInfoAccessFlags::SYNCHRONIZED
        );

        for m in &cf.methods {
            assert_eq!(m.attributes.len(), 1);
            assert!(matches!(m.attributes[0], AttributeInfo::Code { .. }));
        }
        let AttributeInfo::Code { max_locals, .. } = &cf.methods[2].attributes[0] else {
            unreachable!()
        };
        // this + int + long + double, with the wide types taking two slots.
        assert_eq!(*max_locals, 1 + 1 + 2 + 2);
    });
}

#[test]
fn abstract_and_native_methods_without_code_accepted() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "a", "()V", 0);
    cb.add_method(acc::PUBLIC | acc::NATIVE, "n", "()V", 0);
    let bytes = cb.to_bytes();

    assert_parses(&bytes, |cf| {
        assert_eq!(cf.methods.len(), 2);
        assert!(cf.methods[0].attributes.is_empty());
        assert!(cf.methods[1].attributes.is_empty());
    });
}

#[test]
fn method_more_than_one_visibility_rejected() {
    assert_rejected_with!(
        &method_class(52, CLASS_FLAGS, acc::PUBLIC | acc::PRIVATE, "m", "()V"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &method_class(52, CLASS_FLAGS, acc::PROTECTED | acc::PRIVATE, "m", "()V"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &method_class(52, CLASS_FLAGS, acc::PUBLIC | acc::PROTECTED, "m", "()V"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
}

#[test]
fn method_abstract_with_forbidden_flags_rejected() {
    // An abstract method has no body, so nothing that describes a body or
    // forbids overriding may be set (JVMS 4.6).
    for forbidden in [acc::PRIVATE, acc::STATIC, acc::FINAL, acc::SYNCHRONIZED, acc::NATIVE] {
        let bytes =
            method_class(52, ABSTRACT_CLASS_FLAGS, acc::ABSTRACT | forbidden, "m", "()V");
        assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAccessFlagsCombination);
    }
}

#[test]
fn method_abstract_strict_depends_on_version() {
    // ACC_STRICT may not accompany ACC_ABSTRACT in versions 46..60; from 61
    // strictfp is always on and the bit has no meaning, so it is ignored.
    assert_rejected_with!(
        &method_class(
            52,
            ABSTRACT_CLASS_FLAGS,
            acc::PUBLIC | acc::ABSTRACT | acc::STRICT,
            "m",
            "()V",
        ),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_parses(
        &method_class(61, ABSTRACT_CLASS_FLAGS, acc::PUBLIC | acc::ABSTRACT | acc::STRICT, "m", "()V"),
        |cf| assert_eq!(cf.methods.len(), 1),
    );
}

#[test]
fn method_all_legal_flags_accepted() {
    let flags = acc::PUBLIC
        | acc::STATIC
        | acc::FINAL
        | acc::SYNCHRONIZED
        | acc::BRIDGE
        | acc::VARARGS
        | acc::STRICT
        | acc::SYNTHETIC;
    assert_parses(&method_class(52, CLASS_FLAGS, flags, "m", "()V"), |cf| {
        assert_eq!(cf.methods.len(), 1);
    });
}

#[test]
fn method_reserved_flags_ignored() {
    let flags = acc::PUBLIC | 0x0200 | 0x2000 | 0x4000 | 0x8000;
    assert_parses(&method_class(52, CLASS_FLAGS, flags, "m", "()V"), |cf| {
        assert_eq!(cf.methods[0].access_flags, MethodInfoAccessFlags::PUBLIC);
    });
}

#[test]
fn interface_public_abstract_method_accepted() {
    assert_parses(
        &method_class(52, INTERFACE_FLAGS, acc::PUBLIC | acc::ABSTRACT, "m", "()V"),
        |cf| assert_eq!(cf.methods.len(), 1),
    );
}

#[test]
fn interface_default_static_and_private_methods_accepted_from_52() {
    // Default and static interface methods arrived in 52, private ones in 53.
    assert_parses(&method_class(52, INTERFACE_FLAGS, acc::PUBLIC, "d", "()V"), |cf| {
        assert_eq!(cf.methods.len(), 1)
    });
    assert_parses(&method_class(52, INTERFACE_FLAGS, acc::PUBLIC | acc::STATIC, "s", "()V"), |cf| {
        assert_eq!(cf.methods.len(), 1)
    });
    assert_parses(&method_class(53, INTERFACE_FLAGS, acc::PRIVATE, "p", "()V"), |cf| {
        assert_eq!(cf.methods.len(), 1)
    });
}

#[test]
fn interface_method_must_be_public_abstract_before_52() {
    assert_parses(
        &method_class(51, INTERFACE_FLAGS, acc::PUBLIC | acc::ABSTRACT, "m", "()V"),
        |cf| assert_eq!(cf.methods.len(), 1),
    );
    assert_rejected_with!(
        &method_class(51, INTERFACE_FLAGS, acc::PUBLIC, "m", "()V"),
        ClassParserError::ClassParseInvalidFeatureUsedForVersion
    );
    assert_rejected_with!(
        &method_class(51, INTERFACE_FLAGS, acc::PUBLIC | acc::STATIC, "m", "()V"),
        ClassParserError::ClassParseInvalidFeatureUsedForVersion
    );
    // Private interface methods are only legal from 53.
    assert_rejected_with!(
        &method_class(52, INTERFACE_FLAGS, acc::PRIVATE, "p", "()V"),
        ClassParserError::ClassParseInvalidFeatureUsedForVersion
    );
}

#[test]
fn interface_method_forbidden_flags_rejected() {
    assert_rejected_with!(
        &method_class(52, INTERFACE_FLAGS, acc::PROTECTED | acc::ABSTRACT, "m", "()V"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &method_class(52, INTERFACE_FLAGS, acc::PUBLIC | acc::FINAL, "m", "()V"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &method_class(52, INTERFACE_FLAGS, acc::PUBLIC | acc::SYNCHRONIZED, "m", "()V"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
    assert_rejected_with!(
        &method_class(52, INTERFACE_FLAGS, acc::PUBLIC | acc::NATIVE, "m", "()V"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
}

#[test]
fn interface_method_without_public_or_private_rejected() {
    // Package-private interface methods do not exist; exactly one of
    // ACC_PUBLIC and ACC_PRIVATE must be set (JVMS 4.6).
    assert_rejected_with!(
        &method_class(52, INTERFACE_FLAGS, acc::ABSTRACT, "m", "()V"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
}

#[test]
fn init_method_flags() {
    // <init> may only be public/private/protected plus varargs, strict and
    // synthetic; it is never static, final, synchronized, bridge, native or
    // abstract (JVMS 4.6).
    assert_parses(
        &method_class(
            52,
            CLASS_FLAGS,
            acc::PRIVATE | acc::VARARGS | acc::SYNTHETIC,
            "<init>",
            "(I[I)V",
        ),
        |cf| assert_eq!(cf.methods.len(), 1),
    );
    for forbidden in [
        acc::STATIC,
        acc::FINAL,
        acc::SYNCHRONIZED,
        acc::BRIDGE,
        acc::NATIVE,
        acc::ABSTRACT,
    ] {
        let bytes = method_class(52, CLASS_FLAGS, acc::PUBLIC | forbidden, "<init>", "()V");
        assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAccessFlagsCombination);
    }
}

#[test]
fn init_method_must_return_void() {
    assert_rejected_with!(
        &method_class(52, CLASS_FLAGS, acc::PUBLIC, "<init>", "()I"),
        ClassParserError::ClassParseFormatError
    );
}

#[test]
fn init_method_in_interface_rejected() {
    // An interface has no instances, so it has no constructors; only <clinit>
    // is allowed to have a name in angle brackets there.
    assert_rejected_with!(
        &method_class(52, INTERFACE_FLAGS, acc::PUBLIC, "<init>", "()V"),
        ClassParserError::ClassParseFormatError
    );
}

#[test]
fn clinit_method() {
    assert_parses(&method_class(52, CLASS_FLAGS, acc::STATIC, "<clinit>", "()V"), |cf| {
        assert_utf8_eq(cf.methods[0].name, "<clinit>");
    });
    assert_parses(&method_class(52, INTERFACE_FLAGS, acc::STATIC, "<clinit>", "()V"), |cf| {
        assert_utf8_eq(cf.methods[0].name, "<clinit>");
    });
}

// JVMS 2.9.2 only calls a static, no-argument <clinit> an initialization method
// and says other <clinit> methods are "of no consequence". HotSpot rejects them
// with ClassFormatError anyway; these two tests follow HotSpot.
#[test]
fn clinit_must_be_static_from_version_51() {
    assert_rejected_with!(
        &method_class(51, CLASS_FLAGS, 0, "<clinit>", "()V"),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
}

#[test]
fn clinit_with_arguments_rejected() {
    assert_rejected_with!(
        &method_class(52, CLASS_FLAGS, acc::STATIC, "<clinit>", "(I)V"),
        ClassParserError::ClassParseFormatError
    );
}

#[test]
fn clinit_must_return_void() {
    // The angle-bracket names are the two exceptions to the name rules, and
    // both of them constrain the descriptor's return type.
    assert_rejected_with!(
        &method_class(52, CLASS_FLAGS, acc::STATIC, "<clinit>", "()I"),
        ClassParserError::ClassParseFormatError
    );
}

#[test]
fn method_invalid_names_rejected() {
    // Method names may not contain < or > unless they are exactly <init> or <clinit>.
    for name in ["", "a.b", "a;b", "a[b", "a/b", "<m>", "a<b", "<init", "init>", "<clinit"] {
        assert_rejected_with!(
            &method_class(52, CLASS_FLAGS, acc::PUBLIC, name, "()V"),
            ClassParserError::ClassParseFormatError
        );
    }
}

#[test]
fn method_valid_descriptors_accepted() {
    let descs = [
        "()V",
        "(I)I",
        "(JD)J",
        "([Ljava/lang/String;)V",
        "(Ljava/lang/Object;[[I)[Z",
        "(BCDFIJSZ)Ljava/lang/String;",
    ];
    for desc in descs {
        let bytes = method_class(52, CLASS_FLAGS, acc::PUBLIC | acc::STATIC, "m", desc);
        assert_parses(&bytes, |cf| assert_utf8_eq(cf.methods[0].descriptor, desc));
    }
}

#[test]
fn method_invalid_descriptors_rejected() {
    let descs = [
        "",
        "V",
        "I",                     // a field descriptor, not a method one
        "(",
        "()",                    // no return type
        "(V)V",                  // void is not a parameter type
        "(I",
        ")V",
        "()VV",                  // trailing junk after the return type
        "(Ljava/lang/Object)V",  // unterminated class name
        "(L;)V",
        "()[V",
        "(I)Q",
    ];
    for desc in descs {
        // Written abstract so a bad descriptor is the only thing wrong: a Code
        // attribute would need a max_locals derived from that descriptor.
        let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
        cb.access_flags = ABSTRACT_CLASS_FLAGS;
        cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", desc, 0);
        assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
    }
}

#[test]
fn method_descriptor_array_of_256_dimensions_rejected() {
    // The 255-dimension cap applies to array types inside a method descriptor
    // just as it does to a field's (JVMS 4.3.2).
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_method(
        acc::PUBLIC | acc::ABSTRACT,
        "m",
        &format!("({}I)V", "[".repeat(256)),
        0,
    );
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);

    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_method(
        acc::PUBLIC | acc::ABSTRACT,
        "m",
        &format!("({}I)V", "[".repeat(255)),
        0,
    );
    assert_parses(&cb.to_bytes(), |cf| assert_eq!(cf.methods.len(), 1));
}

#[test]
fn method_with_more_than_255_parameter_slots_rejected() {
    // A method descriptor is only valid if its parameters need <= 255 slots
    // (including `this` for instance methods).
    let desc = format!("({})V", "J".repeat(128)); // 256 slots
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", &desc, 0);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn method_with_exactly_255_parameter_slots_accepted() {
    // 127 longs plus an int is 255 slots: the limit is inclusive, and for a
    // static method nothing else is counted.
    let desc = format!("({}I)V", "J".repeat(127));
    let bytes = method_class(52, CLASS_FLAGS, acc::PUBLIC | acc::STATIC, "m", &desc);
    assert_parses(&bytes, |cf| assert_eq!(cf.methods.len(), 1));
}

#[test]
fn instance_method_parameter_slots_include_this() {
    // The same 255-slot descriptor that a static method may have overflows on
    // an instance method, because `this` occupies the first slot.
    let at_limit = format!("({})V", "J".repeat(127)); // 254 + this = 255
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", &at_limit, 0);
    assert_parses(&cb.to_bytes(), |cf| assert_eq!(cf.methods.len(), 1));

    let over = format!("({}I)V", "J".repeat(127)); // 255 + this = 256
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", &over, 0);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn concrete_method_without_code_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC, "m", "()V", 0);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn abstract_method_with_code_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", "()V", 1);
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 0, 1, &[op::RETURN]);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn native_method_with_code_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC | acc::NATIVE, "m", "()V", 1);
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 0, 1, &[op::RETURN]);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn method_with_two_code_attributes_rejected() {
    // JVMS 4.7.3: exactly one Code attribute in a non-abstract, non-native method.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC, "m", "()V", 2);
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 0, 1, &[op::RETURN]);
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 0, 1, &[op::RETURN]);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn method_with_two_exceptions_attributes_rejected() {
    // JVMS 4.7.5: at most one Exceptions attribute.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    let exc = cb.pool.utf8("Exceptions");
    let io = cb.pool.class("java/io/IOException");
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", "()V", 2);
    cb.methods.attr_u2_table(exc, &[io]);
    cb.methods.attr_u2_table(exc, &[io]);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn code_max_locals_smaller_than_arguments_rejected() {
    // The arguments occupy the first local slots, so max_locals can never be
    // less than the slots the descriptor needs (JVMS 4.7.3).
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC | acc::STATIC, "m", "(JI)V", 1);
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 0, 2, &[op::RETURN]); // needs 3

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidCodeAttribute);
}

#[test]
fn code_max_locals_equal_to_arguments_accepted() {
    // The boundary the previous test brackets: exactly enough slots is fine.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC | acc::STATIC, "m", "(JI)V", 1);
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 0, 3, &[op::RETURN]);

    assert_parses(&cb.to_bytes(), |cf| assert_eq!(cf.methods.len(), 1));
}

#[test]
fn code_max_locals_must_count_this_for_instance_methods() {
    // An instance method's slot 0 is `this`, so (JI)V needs four, not three.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_method(acc::PUBLIC, "m", "(JI)V", 1);
    ClassBuilder::write_code_attr(&mut cb.methods, &mut cb.pool, 0, 3, &[op::RETURN]);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidCodeAttribute);
}

#[test]
fn duplicate_method_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_simple_method(acc::PUBLIC, "m", "()V");
    cb.add_simple_method(acc::PRIVATE, "m", "()V");

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn overloaded_methods_accepted() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_simple_method(acc::PUBLIC, "m", "()V");
    cb.add_simple_method(acc::PUBLIC, "m", "(I)V");
    cb.add_simple_method(acc::PUBLIC, "m", "()I"); // return-type overload is legal too

    assert_parses(&cb.to_bytes(), |cf| {
        assert_eq!(cf.methods.len(), 3);
        for desc in ["()V", "(I)V", "()I"] {
            assert!(
                cf.methods
                    .iter()
                    .any(|m| m.name.as_bytes() == b"m" && m.descriptor.as_bytes() == desc.as_bytes()),
                "overload m{desc} is missing"
            );
        }
    });
}

#[test]
fn method_name_index_not_utf8_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    let name = cb.pool.integer(5);
    let desc = cb.pool.utf8("()V");
    cb.methods.u2(acc::PUBLIC | acc::ABSTRACT);
    cb.methods.u2(name);
    cb.methods.u2(desc);
    cb.methods.u2(0);
    cb.methods_count += 1;

    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn method_descriptor_index_out_of_range_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    let name = cb.pool.utf8("m");
    cb.methods.u2(acc::PUBLIC | acc::ABSTRACT);
    cb.methods.u2(name);
    cb.methods.u2(4000);
    cb.methods.u2(0);
    cb.methods_count += 1;

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn method_descriptor_index_zero_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    let name = cb.pool.utf8("m");
    cb.methods.u2(acc::PUBLIC | acc::ABSTRACT);
    cb.methods.u2(name);
    cb.methods.u2(0);
    cb.methods.u2(0);
    cb.methods_count += 1;

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn method_descriptor_index_unusable_long_slot_rejected() {
    // The slot after a Long is in range but names no entry (JVMS 4.4.5).
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = ABSTRACT_CLASS_FLAGS;
    let name = cb.pool.utf8("m");
    let long = cb.pool.long(0);
    cb.methods.u2(acc::PUBLIC | acc::ABSTRACT);
    cb.methods.u2(name);
    cb.methods.u2(long + 1);
    cb.methods.u2(0);
    cb.methods_count += 1;

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn methods_count_larger_than_data_rejected() {
    // Only the two bytes of attributes_count follow the method, so the second
    // method_info runs off the end before any of its fields can be validated.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_simple_method(acc::PUBLIC | acc::STATIC, "m", "()V");
    cb.methods_count = 2;

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::Truncated(_));
}

#[test]
fn many_methods() {
    // A thousand methods is well within the u2 count but enough to catch a
    // parser that reallocates or indexes badly as the list grows.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    for i in 0..1000 {
        cb.add_simple_method(acc::PUBLIC | acc::STATIC, &format!("m{i}"), "()V");
    }

    assert_parses(&cb.to_bytes(), |cf| {
        assert_eq!(cf.methods.len(), 1000);
        assert!(cf.methods.iter().any(|m| m.name.as_bytes() == b"m999"));
    });
}
