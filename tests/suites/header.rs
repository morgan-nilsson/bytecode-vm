//! The ClassFile structure (JVMS 4.1): magic, versions, constant pool count,
//! access flags, this/super class, interfaces, and whole-file framing
//! (truncation and trailing bytes, JVMS 4.8).
//!
//! Ported from `c_backup/tests/test_classfile_header.c`.


use crate::common::*;

use bytecode_vm::java_utf::JavaUTF8;
use bytecode_vm::parser::class_file::{
    ClassFile, ClassFileAccessFlags, ClassParserError, ConstantPoolEntry,
};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// The CONSTANT_Utf8 at `index`. Compares equal to a `&str` directly.
#[track_caller]
fn utf8_at<'a>(cf: &ClassFile<'a>, index: u16) -> JavaUTF8<'a> {
    match cf.constant(index) {
        Some(ConstantPoolEntry::UTF8(s)) => *s,
        _ => panic!("constant {index} is not a CONSTANT_Utf8"),
    }
}

/// The `name_index` of the CONSTANT_Class at `index`.
#[track_caller]
fn class_name_index(cf: &ClassFile<'_>, index: u16) -> u16 {
    match cf.constant(index) {
        Some(ConstantPoolEntry::ClassIndex(n)) => *n,
        _ => panic!("constant {index} is not a CONSTANT_Class"),
    }
}

fn version_bytes(major: u16, minor: u16) -> Vec<u8> {
    let mut cb = ClassBuilder::new(major);
    cb.minor_version = minor;
    cb.to_bytes()
}

/// Asserts the version pair is accepted and survives the round trip.
#[track_caller]
fn assert_version_accepted(major: u16, minor: u16) {
    let bytes = version_bytes(major, minor);
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.major_version, major, "major version {major}.{minor} changed in parsing");
        assert_eq!(cf.minor_version, minor, "minor version {major}.{minor} changed in parsing");
    });
}

#[track_caller]
fn assert_version_rejected(major: u16, minor: u16) {
    let bytes = version_bytes(major, minor);
    let err = assert_rejected(&bytes);
    assert!(
        matches!(err, ClassParserError::ClassParseUnsupportedVersion),
        "expected {major}.{minor} to be an unsupported version, got {err:?}"
    );
}

fn class_with_flags(flags: u16) -> Vec<u8> {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = flags;
    cb.to_bytes()
}

/// Asserts the access flags are accepted; unassigned bits are dropped, since
/// JVMS 4.1 says implementations must ignore them.
#[track_caller]
fn assert_flags_accepted(flags: u16) {
    let bytes = class_with_flags(flags);
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.access_flags, ClassFileAccessFlags::from_bits_truncate(flags));
    });
}

/// Asserts the access flags are an illegal combination, which is a plain JVMS
/// 4.8 format check rather than anything structural.
#[track_caller]
fn assert_flags_rejected(flags: u16) {
    assert_rejected_with!(
        &class_with_flags(flags),
        ClassParserError::ClassParseInvalidAccessFlagsCombination
    );
}

// ---------------------------------------------------------------------------
// Successful parse of the smallest valid class
// ---------------------------------------------------------------------------

#[test]
fn minimal_class_parses() {
    let bytes = ClassBuilder::new(DEFAULT_MAJOR_VERSION).to_bytes();
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.minor_version, 0);
        assert_eq!(cf.major_version, 52);
        // constant_pool_count is 5: the pool holds count - 1 entries.
        assert_eq!(cf.constant_pool.len(), 4);
        assert_eq!(cf.access_flags, ClassFileAccessFlags::PUBLIC | ClassFileAccessFlags::SUPER);
        assert_utf8_eq(cf.this_class, "Test");
        assert_utf8_eq(cf.super_class.expect("super_class should resolve"), "java/lang/Object");
        assert!(cf.interfaces.is_empty());
        assert!(cf.fields.is_empty());
        assert!(cf.methods.is_empty());
        assert!(cf.attributes.is_empty());
    });
}

#[test]
fn minimal_class_constant_pool_contents() {
    let bytes = ClassBuilder::new(DEFAULT_MAJOR_VERSION).to_bytes();
    assert_parses(&bytes, |cf| {
        assert_eq!(utf8_at(cf, 1), "Test");
        assert_eq!(class_name_index(cf, 2), 1);
        assert_eq!(utf8_at(cf, 3), "java/lang/Object");
        assert_eq!(class_name_index(cf, 4), 3);
    });
}

#[test]
fn constant_pool_indices_are_one_based() {
    // Index 0 is not a pool entry, and one past the end is out of range.
    let bytes = ClassBuilder::new(DEFAULT_MAJOR_VERSION).to_bytes();
    assert_parses(&bytes, |cf| {
        assert!(cf.constant(0).is_none());
        assert!(cf.constant(5).is_none());
        assert!(cf.constant(u16::MAX).is_none());
    });
}

#[test]
fn slot_after_a_long_or_double_is_unusable() {
    // JVMS 4.4.5: an eight-byte constant takes two slots and the second one
    // must never resolve, even though the pool has room for it.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let long = cb.pool.long(0x0102_0304_0506_0708);
    let double = cb.pool.double_bits(0x4009_21FB_5444_2D18);
    let bytes = cb.to_bytes();
    assert_parses(&bytes, |cf| {
        assert!(matches!(cf.constant(long), Some(ConstantPoolEntry::Long(0x0102_0304_0506_0708))));
        assert!(cf.constant(long + 1).is_none());
        assert!(matches!(cf.constant(double), Some(ConstantPoolEntry::Double(_))));
        assert!(cf.constant(double + 1).is_none());
    });
}

#[test]
fn parsing_twice_gives_independent_results() {
    let a = ClassBuilder::new(DEFAULT_MAJOR_VERSION).to_bytes();

    let mut b = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    b.access_flags = acc::PUBLIC | acc::SUPER;
    b.this_class = b.pool.class("Other");
    b.super_class = b.pool.class("java/lang/Object");
    let b = b.to_bytes();

    assert_parses(&a, |cf| assert_utf8_eq(cf.this_class, "Test"));
    assert_parses(&b, |cf| assert_utf8_eq(cf.this_class, "Other"));
    // The first result must not have been disturbed by the second parse.
    assert_parses(&a, |cf| assert_utf8_eq(cf.this_class, "Test"));
}

// ---------------------------------------------------------------------------
// Magic
// ---------------------------------------------------------------------------

#[test]
fn empty_file_is_rejected() {
    // Nothing to compare against the magic, so this is running off the end
    // rather than a bad magic.
    assert_rejected_with!(&[], ClassParserError::Truncated(_));
}

#[test]
fn bad_magic_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.magic = 0xCAFE_BABF;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidMagic);
}

#[test]
fn byte_swapped_magic_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.magic = 0xBEBA_FECA;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidMagic);
}

#[test]
fn zero_magic_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.magic = 0;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidMagic);
}

#[test]
fn magic_only_is_rejected() {
    // The magic is the one thing that is right, so the file simply ends early.
    assert_rejected_with!(&[0xCA, 0xFE, 0xBA, 0xBE], ClassParserError::Truncated(_));
}

#[test]
fn partial_magic_is_rejected() {
    // Too short to decide whether the magic is wrong, so it cannot be a magic
    // complaint.
    assert_rejected_with!(&[0xCA, 0xFE], ClassParserError::Truncated(_));
}

#[test]
fn text_file_is_rejected() {
    assert_rejected_with!(b"public class Test {}\n", ClassParserError::ClassParseInvalidMagic);
}

// ---------------------------------------------------------------------------
// Versions (JVMS 4.1: minor_version, major_version)
// ---------------------------------------------------------------------------

#[test]
fn version_45_0_jdk_1_0_accepted() {
    assert_version_accepted(45, 0);
}

#[test]
fn version_45_3_jdk_1_1_accepted() {
    assert_version_accepted(45, 3);
}

#[test]
fn version_46_through_55_accepted() {
    for major in 46..=55 {
        assert_version_accepted(major, 0);
    }
}

#[test]
fn version_below_56_accepts_any_minor() {
    // Before Java 12 every minor_version is valid.
    assert_version_accepted(50, 1);
    assert_version_accepted(52, 0x7fff);
    assert_version_accepted(55, 65535);
}

#[test]
fn version_56_0_accepted() {
    assert_version_accepted(56, 0);
}

#[test]
fn version_57_through_max_accepted() {
    for major in 57..=MAX_MAJOR_VERSION {
        assert_version_accepted(major, 0);
    }
}

#[test]
fn version_61_0_java_17_accepted() {
    assert_version_accepted(61, 0);
}

#[test]
fn version_max_accepted() {
    assert_version_accepted(MAX_MAJOR_VERSION, 0);
}

#[test]
fn version_56_nonzero_minor_rejected() {
    // From 56 onwards minor_version must be 0 or 65535.
    assert_version_rejected(56, 1);
}

#[test]
fn version_61_nonzero_minor_rejected() {
    assert_version_rejected(61, 1);
    assert_version_rejected(61, 3);
    assert_version_rejected(61, 65534);
}

#[test]
fn version_preview_minor_of_older_release_rejected() {
    // minor 65535 marks a preview-feature class file, which a JVM may only
    // load when it matches its own feature release with previews enabled.
    assert_version_rejected(61, 65535);
}

#[test]
fn version_max_preview_minor_rejected() {
    // Preview classes are not loadable even at the newest release this parser
    // knows: enabling previews is a launch-time decision the parser can't make.
    assert_version_rejected(MAX_MAJOR_VERSION, 65535);
}

#[test]
fn version_44_rejected() {
    assert_version_rejected(44, 0);
}

#[test]
fn version_0_rejected() {
    assert_version_rejected(0, 0);
}

#[test]
fn version_above_max_rejected() {
    assert_version_rejected(MAX_MAJOR_VERSION + 1, 0);
}

#[test]
fn version_0xffff_rejected() {
    assert_version_rejected(0xffff, 0);
}

#[test]
fn version_bytes_are_minor_then_major() {
    // 0x0034 in the minor slot and 0x0000 in the major slot is 0.52, not 52.0.
    assert_version_rejected(0, 52);
}

// ---------------------------------------------------------------------------
// constant_pool_count
// ---------------------------------------------------------------------------

#[test]
fn constant_pool_count_zero_rejected() {
    // constant_pool_count must be at least 1; 0 must not wrap to 65535 entries.
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.pool_count_override = Some(0);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPool);
}

#[test]
fn constant_pool_count_one_is_empty_pool() {
    // An empty pool is structurally fine, but then this_class can't be valid.
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::SUPER;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidThisClassIndex);
}

#[test]
fn constant_pool_count_larger_than_entries_rejected() {
    // Deliberately unpinned. The surplus entries are read out of whatever
    // follows the pool, so which check fires depends on where those bytes
    // land rather than on anything the contract settles.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool_count_override = Some(cb.pool.next + 3);
    assert_rejected(&cb.to_bytes());
}

#[test]
fn constant_pool_count_smaller_than_entries_rejected() {
    // With the count one short, the last Class entry's tag byte is read as
    // access_flags and the rest of the file is misaligned. Which check trips
    // first depends on where the shifted bytes land, so only rejection matters.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.pool_count_override = Some(cb.pool.next - 1);
    assert_rejected(&cb.to_bytes());
}

#[test]
fn large_constant_pool() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    while cb.pool.next < 60000 {
        let s = format!("s{}", cb.pool.next);
        cb.pool.utf8(&s);
    }
    let bytes = cb.to_bytes();
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.constant_pool.len(), 59999);
        assert_eq!(utf8_at(cf, 59999), "s59999");
    });
}

#[test]
fn maximum_constant_pool_count_65535() {
    // 65535 is the largest count the u2 field can hold, so the last usable
    // index is 65534.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    while cb.pool.next < 65535 {
        let v = i32::from(cb.pool.next);
        cb.pool.integer(v);
    }
    let bytes = cb.to_bytes();
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.constant_pool.len(), 65534);
        assert!(matches!(cf.constant(65534), Some(ConstantPoolEntry::Integer(65534))));
    });
}

// ---------------------------------------------------------------------------
// Class access flags (JVMS 4.1, table 4.1-B)
// ---------------------------------------------------------------------------

#[test]
fn class_access_flag_bits() {
    for (flag, bits) in [
        (ClassFileAccessFlags::PUBLIC, 0x0001u16),
        (ClassFileAccessFlags::FINAL, 0x0010),
        (ClassFileAccessFlags::SUPER, 0x0020),
        (ClassFileAccessFlags::INTERFACE, 0x0200),
        (ClassFileAccessFlags::ABSTRACT, 0x0400),
        (ClassFileAccessFlags::SYNTHETIC, 0x1000),
        (ClassFileAccessFlags::ANNOTATION, 0x2000),
        (ClassFileAccessFlags::ENUM, 0x4000),
        (ClassFileAccessFlags::MODULE, 0x8000),
    ] {
        assert_eq!(flag.bits(), bits);
        assert!(ClassFileAccessFlags::from_bits_truncate(bits).contains(flag));
        // Every other bit set: the flag must not be read out of its neighbours.
        assert!(!ClassFileAccessFlags::from_bits_truncate(!bits).contains(flag));
    }
}

#[test]
fn class_flags_final_accepted() {
    assert_flags_accepted(acc::PUBLIC | acc::FINAL | acc::SUPER);
}

#[test]
fn class_flags_abstract_accepted() {
    assert_flags_accepted(acc::PUBLIC | acc::ABSTRACT | acc::SUPER);
}

#[test]
fn class_flags_package_private_accepted() {
    assert_flags_accepted(acc::SUPER);
}

#[test]
fn class_flags_synthetic_enum_accepted() {
    assert_flags_accepted(acc::PUBLIC | acc::FINAL | acc::SUPER | acc::ENUM | acc::SYNTHETIC);
}

#[test]
fn class_flags_reserved_bits_ignored() {
    // Unassigned bits "should be ignored by Java Virtual Machine implementations".
    assert_flags_accepted(acc::PUBLIC | acc::SUPER | 0x0002 | 0x0004 | 0x0100 | 0x0800);
}

#[test]
fn class_flags_interface_abstract_accepted() {
    assert_flags_accepted(acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT);
}

#[test]
fn class_flags_annotation_accepted() {
    assert_flags_accepted(acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT | acc::ANNOTATION);
}

#[test]
fn class_flags_interface_without_abstract_rejected() {
    assert_flags_rejected(acc::PUBLIC | acc::INTERFACE);
}

#[test]
fn class_flags_interface_final_rejected() {
    assert_flags_rejected(acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT | acc::FINAL);
}

#[test]
fn class_flags_interface_super_rejected() {
    assert_flags_rejected(acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT | acc::SUPER);
}

#[test]
fn class_flags_interface_enum_rejected() {
    assert_flags_rejected(acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT | acc::ENUM);
}

#[test]
fn class_flags_annotation_without_interface_rejected() {
    assert_flags_rejected(acc::PUBLIC | acc::SUPER | acc::ANNOTATION);
}

#[test]
fn class_flags_final_and_abstract_rejected() {
    assert_flags_rejected(acc::PUBLIC | acc::SUPER | acc::FINAL | acc::ABSTRACT);
}

#[test]
fn class_flags_interface_must_be_abstract() {
    // The positive half of the interface rules: ACC_ABSTRACT is not merely
    // permitted alongside ACC_INTERFACE, it is required, so the combination
    // every real interface carries has to be accepted.
    assert_flags_accepted(acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT);
    assert_flags_accepted(acc::INTERFACE | acc::ABSTRACT);
}

#[test]
fn class_flags_interface_annotation_without_abstract_rejected() {
    // An annotation interface is still an interface, so it needs ACC_ABSTRACT.
    assert_flags_rejected(acc::PUBLIC | acc::INTERFACE | acc::ANNOTATION);
}

#[test]
fn class_flags_enum_and_interface_rejected() {
    assert_flags_rejected(acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT | acc::ENUM);
}

#[test]
fn class_flags_module_on_regular_class_rejected() {
    // ACC_MODULE requires the module-info shape (see the module tests).
    // Version 53 because the flag was unassigned before modules existed.
    let mut cb = ClassBuilder::new(53);
    cb.access_flags = acc::MODULE;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

// ---------------------------------------------------------------------------
// this_class / super_class
// ---------------------------------------------------------------------------

#[test]
fn this_class_zero_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.this_class = 0;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidThisClassIndex);
}

#[test]
fn this_class_out_of_range_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.this_class = cb.pool.next;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidThisClassIndex);
}

#[test]
fn this_class_far_out_of_range_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.this_class = 0xffff;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidThisClassIndex);
}

#[test]
fn this_class_not_a_class_entry_rejected() {
    // A usable index of the wrong kind is still a this_class problem, not the
    // general wrong-kind reference error.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.this_class = 1; // the Utf8 "Test", not the Class
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidThisClassIndex);
}

#[test]
fn this_class_pointing_at_an_unusable_slot_rejected() {
    // The second slot of a Long is not an entry at all, so it can't be a class.
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::SUPER;
    let long = cb.pool.long(1);
    cb.this_class = long + 1;
    cb.super_class = cb.pool.class("java/lang/Object");
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidThisClassIndex);
}

#[test]
fn super_class_out_of_range_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.super_class = 200;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidSuperClassIndex);
}

#[test]
fn super_class_not_a_class_entry_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.super_class = cb.pool.integer(7);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidSuperClassIndex);
}

#[test]
fn super_class_pointing_at_an_unusable_slot_rejected() {
    // Same hole as for this_class: the slot after a Long names no entry, and a
    // super_class that is neither 0 nor a usable Class index is an index error.
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::SUPER;
    cb.this_class = cb.pool.class("Test");
    let long = cb.pool.long(1);
    cb.super_class = long + 1;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidSuperClassIndex);
}

#[test]
fn super_class_zero_for_ordinary_class_rejected() {
    // Only java/lang/Object (and module-info) may have super_class 0, so for
    // any other class the zero is what makes the index unusable.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.super_class = 0;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidSuperClassIndex);
}

#[test]
fn super_class_zero_for_java_lang_object_accepted() {
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::SUPER;
    cb.this_class = cb.pool.class("java/lang/Object");
    cb.super_class = 0;
    let bytes = cb.to_bytes();
    assert_parses(&bytes, |cf| {
        assert_utf8_eq(cf.this_class, "java/lang/Object");
        assert!(cf.super_class.is_none(), "super_class 0 should resolve to no superclass");
    });
}

#[test]
fn this_class_name_in_dotted_form_rejected() {
    // Class names in the constant pool use the internal form with '/'.
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::SUPER;
    cb.this_class = cb.pool.class("com.example.Test");
    cb.super_class = cb.pool.class("java/lang/Object");
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn this_class_in_package_accepted() {
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::SUPER;
    cb.this_class = cb.pool.class("com/example/deep/Test$Inner");
    cb.super_class = cb.pool.class("java/lang/Object");
    let bytes = cb.to_bytes();
    assert_parses(&bytes, |cf| {
        assert_utf8_eq(cf.this_class, "com/example/deep/Test$Inner");
    });
}

#[test]
fn array_class_name_rejected() {
    // A class file may not describe an array type: its name has to be a
    // binary name in internal form, never a descriptor (JVMS 4.2.1).
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::SUPER;
    cb.this_class = cb.pool.class("[Ljava/lang/Object;");
    cb.super_class = cb.pool.class("java/lang/Object");
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}

#[test]
fn interface_super_class_must_be_object() {
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT;
    cb.this_class = cb.pool.class("Test");
    cb.super_class = cb.pool.class("java/lang/Number");
    // A perfectly good Class index, but an interface may only extend Object,
    // so it reports the same way as an interface with no super class at all.
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidSuperClassIndex);
}

#[test]
fn interface_super_class_zero_rejected() {
    // An interface's super_class must be a valid index naming Object, so 0 is
    // not allowed even though an ordinary root class may use it.
    let mut cb = ClassBuilder::empty(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT;
    cb.this_class = cb.pool.class("Test");
    cb.super_class = 0;
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidSuperClassIndex);
}

// ---------------------------------------------------------------------------
// Interfaces
// ---------------------------------------------------------------------------

#[test]
fn interfaces_parsed_in_order() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let runnable = cb.pool.class("java/lang/Runnable");
    let cloneable = cb.pool.class("java/lang/Cloneable");
    let serializable = cb.pool.class("java/io/Serializable");
    cb.add_interface(runnable);
    cb.add_interface(cloneable);
    cb.add_interface(serializable);

    let bytes = cb.to_bytes();
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.interfaces.len(), 3);
        assert_utf8_eq(cf.interfaces[0], "java/lang/Runnable");
        assert_utf8_eq(cf.interfaces[1], "java/lang/Cloneable");
        assert_utf8_eq(cf.interfaces[2], "java/io/Serializable");
    });
}

#[test]
fn interface_index_zero_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_interface(0);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn interface_index_out_of_range_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.add_interface(999);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn interface_not_a_class_entry_rejected() {
    // The index is usable, so the complaint is about the kind of entry it
    // names, not about the index.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let utf8 = cb.pool.utf8("java/lang/Runnable");
    cb.add_interface(utf8);
    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn interfaces_count_larger_than_data_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let runnable = cb.pool.class("java/lang/Runnable");
    cb.add_interface(runnable);
    // Whether this reads a bad index out of the counts that follow or runs off
    // the end first is up to the parser; either way the class is not loadable.
    cb.interfaces_count = 40; // only one index actually follows
    assert_rejected(&cb.to_bytes());
}

// ---------------------------------------------------------------------------
// Framing: truncation and trailing data (JVMS 4.8)
// ---------------------------------------------------------------------------

/// A class with at least one of everything, used to cut at every offset.
fn rich_class_bytes() -> Vec<u8> {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let runnable = cb.pool.class("java/lang/Runnable");
    cb.add_interface(runnable);
    cb.pool.long(0x0102_0304_0506_0708);
    cb.pool.double_bits(0x4009_21FB_5444_2D18);
    cb.pool.string("hello");

    cb.add_field(acc::PRIVATE | acc::STATIC | acc::FINAL, "X", "I", 1);
    let constant_value = cb.pool.utf8("ConstantValue");
    let forty_two = cb.pool.integer(42);
    cb.fields.attr_u2(constant_value, forty_two);

    cb.add_simple_method(acc::PUBLIC, "<init>", "()V");
    cb.add_simple_method(acc::PUBLIC, "run", "()V");

    cb.reserve_attributes(1);
    let source_file = cb.pool.utf8("SourceFile");
    let file_name = cb.pool.utf8("Test.java");
    cb.attributes.attr_u2(source_file, file_name);

    cb.to_bytes()
}

#[test]
fn rich_class_parses() {
    let bytes = rich_class_bytes();
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.interfaces.len(), 1);
        assert_eq!(cf.fields.len(), 1);
        assert_eq!(cf.methods.len(), 2);
        assert_eq!(cf.attributes.len(), 1);
    });
}

#[test]
fn truncated_at_every_offset_rejected() {
    let full = rich_class_bytes();
    for len in 0..full.len() {
        let result = with_parsed(&full[..len], |r| r.is_err());
        assert!(result, "class truncated to {len} of {} bytes was accepted", full.len());
    }
}

#[test]
fn trailing_byte_rejected() {
    // A single spare byte is enough: the class ends where its last attribute
    // does, so nothing may follow.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.trailing.u1(0);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseTrailingBytes);
}

#[test]
fn trailing_garbage_rejected() {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.trailing.raw(b"garbage");
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseTrailingBytes);
}

#[test]
fn two_concatenated_classes_rejected() {
    // Trailing bytes that are themselves a valid class are still trailing
    // bytes: a file holds exactly one class.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let one = cb.to_bytes();
    cb.trailing.raw(&one);
    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseTrailingBytes);
}

#[test]
fn counts_of_0xffff_with_no_data_do_not_hang_or_crash() {
    // Every count claims 65535 elements but the file ends immediately, so the
    // first element read must run off the end instead of allocating for them.
    let mut bytes = ClassBuilder::new(DEFAULT_MAJOR_VERSION).to_bytes();
    bytes.truncate(bytes.len() - 8); // drop the four zero counts
    bytes.extend_from_slice(&0xffffu16.to_be_bytes());
    assert_rejected_with!(&bytes, ClassParserError::Truncated(_));
}

#[test]
fn garbage_after_valid_header_does_not_crash() {
    // Deterministic pseudo-random bodies after a valid magic and version.
    let mut seed: u32 = 12345;
    for _ in 0..200 {
        let mut b = Bytes::new();
        b.u4(0xCAFE_BABE);
        b.u2(0);
        b.u2(DEFAULT_MAJOR_VERSION);
        let len = 1 + (seed % 300);
        for _ in 0..len {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            b.u1((seed >> 16) as u8);
        }
        // Only that the parser terminates without panicking matters here.
        with_parsed(&b.data, |r| drop(r));
    }
}
