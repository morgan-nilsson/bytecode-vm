//! Real class files produced by javac 17 (tests/fixtures/src, built by
//! tests/fixtures/build.sh). Expected values come from `javap -v -p`.
//!
//! Ported from the C suite in `c_backup/tests/test_classfile_fixtures.c`. Where
//! the C parser kept a constant pool index the Rust parser resolves the name, so
//! those assertions became name assertions; the indices are still in the
//! comments where they help.
//!
//! Also a corpus test: set CLASSFILE_CORPUS to a directory of .class files and
//! every file in it must parse.

use crate::common::*;

use std::path::{Path, PathBuf};

use bytecode_vm::java_utf::JavaUTF8;
use bytecode_vm::parser::class_file::*;
use bytecode_vm::parser::class_file::{
    ClassFileAccessFlags as CF, FieldInfoAccessFlags as FF, InnerClassAccessFlags as IF,
    MethodInfoAccessFlags as MF, MethodParameterAccessFlags as PF, ModuleExportsFlags as EF,
    MethodHandleKind, ModuleFlags as MdF, ModuleRequiresFlags as RF,
};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// The first attribute matching `pattern`, or `None`.
macro_rules! find_attr {
    ($attrs:expr, $pattern:pat => $bound:expr) => {
        $attrs.iter().find_map(|a| match a {
            $pattern => Some($bound),
            _ => None,
        })
    };
}

/// The first attribute matching `pattern`, panicking when there is none.
macro_rules! require_attr {
    ($attrs:expr, $pattern:pat => $bound:expr) => {
        find_attr!($attrs, $pattern => $bound)
            .unwrap_or_else(|| panic!("no {} attribute", stringify!($pattern)))
    };
}

/// `constant_pool_count` as written in the file: the pool is numbered from 1.
fn constant_pool_count(cf: &ClassFile<'_>) -> usize {
    cf.constant_pool.len() + 1
}

/// The CONSTANT_Utf8 at `index`. Compares equal to a `&str` directly.
fn utf8_at<'a>(cf: &ClassFile<'a>, index: u16) -> JavaUTF8<'a> {
    match cf.constant(index) {
        Some(ConstantPoolEntry::UTF8(s)) => *s,
        _ => panic!("#{index} is not a CONSTANT_Utf8"),
    }
}

/// The name a CONSTANT_Class entry points at.
fn class_name_at<'a>(cf: &ClassFile<'a>, index: u16) -> JavaUTF8<'a> {
    match cf.constant(index) {
        Some(ConstantPoolEntry::ClassIndex(name)) => utf8_at(cf, *name),
        _ => panic!("#{index} is not a CONSTANT_Class"),
    }
}

fn find_field<'b, 'a>(cf: &'b ClassFile<'a>, name: &str) -> &'b FieldInfo<'a> {
    cf.fields
        .iter()
        .find(|f| f.name.as_bytes() == name.as_bytes())
        .unwrap_or_else(|| panic!("no field {name}"))
}

/// Finds a method by name, and by descriptor when one is given.
fn find_method<'b, 'a>(
    cf: &'b ClassFile<'a>,
    name: &str,
    descriptor: Option<&str>,
) -> &'b MethodInfo<'a> {
    cf.methods
        .iter()
        .find(|m| {
            m.name.as_bytes() == name.as_bytes()
                && descriptor.is_none_or(|d| m.descriptor.as_bytes() == d.as_bytes())
        })
        .unwrap_or_else(|| panic!("no method {name}"))
}

fn count_unknown(attributes: &[AttributeInfo<'_>]) -> usize {
    attributes.iter().filter(|a| matches!(a, AttributeInfo::Unknown { .. })).count()
}

/// The class name of an `ObjectVariableInfo` verification type.
fn object_class<'a>(cf: &ClassFile<'a>, info: &VerificationTypeInfo) -> JavaUTF8<'a> {
    match info {
        VerificationTypeInfo::ObjectVariableInfo { cpool_index } => class_name_at(cf, *cpool_index),
        _ => panic!("expected an object verification type"),
    }
}

// ---------------------------------------------------------------------------
// Every fixture
// ---------------------------------------------------------------------------

#[test]
fn every_fixture_parses() {
    let mut failures = Vec::new();
    for name in ALL_FIXTURES {
        let bytes = fixture(name);
        with_parsed(&bytes, |result| {
            if let Err(e) = result {
                failures.push(format!("    {name}: {e}"));
            }
        });
    }
    assert!(
        failures.is_empty(),
        "{} of {} fixtures failed to parse:\n{}",
        failures.len(),
        ALL_FIXTURES.len(),
        failures.join("\n")
    );
}

#[test]
fn every_fixture_truncated_by_one_byte_rejected() {
    // Deliberately unpinned: which error comes out depends on what the last
    // byte belonged to — the file running short, or an attribute no longer
    // filling its declared length. Only the rejection itself matters here.
    for name in ALL_FIXTURES {
        let bytes = fixture(name);
        let truncated = &bytes[..bytes.len() - 1];
        with_parsed(truncated, |result| {
            assert!(result.is_err(), "{name} minus its last byte was accepted");
        });
    }
}

// ---------------------------------------------------------------------------
// fixtures/Hello
// ---------------------------------------------------------------------------

#[test]
fn hello_header() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.major_version, 61);
        assert_eq!(cf.minor_version, 0);
        assert_eq!(constant_pool_count(cf), 242);
        assert_eq!(cf.access_flags, CF::PUBLIC | CF::SUPER);
        // this_class #11, super_class #2
        assert_utf8_eq(cf.this_class, "fixtures/Hello");
        assert_utf8_eq(cf.super_class.expect("a super class"), "java/lang/Object");
        assert_eq!(cf.interfaces.len(), 1);
        assert_utf8_eq(cf.interfaces[0], "java/lang/Runnable");
        assert_eq!(cf.fields.len(), 9);
        assert_eq!(cf.methods.len(), 11);
        assert_eq!(cf.attributes.len(), 4);
    });
}

#[test]
fn hello_constant_pool_entries() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        match cf.constant(1) {
            Some(ConstantPoolEntry::MethodRef { class_index, name_and_type_index }) => {
                assert_eq!(*class_index, 2);
                assert_eq!(*name_and_type_index, 3);
            }
            _ => panic!("#1 is not a Methodref"),
        }
        match cf.constant(3) {
            Some(ConstantPoolEntry::NameAndType { name_index, descriptor_index }) => {
                assert_eq!(utf8_at(cf, *name_index), "<init>");
                assert_eq!(utf8_at(cf, *descriptor_index), "()V");
            }
            _ => panic!("#3 is not a NameAndType"),
        }

        match cf.constant(10) {
            Some(ConstantPoolEntry::FieldRef { class_index, name_and_type_index }) => {
                assert_eq!(*class_index, 11);
                assert_eq!(*name_and_type_index, 12);
            }
            _ => panic!("#10 is not a Fieldref"),
        }

        match cf.constant(30) {
            Some(ConstantPoolEntry::InvokeDynamic { bootstrap_index, name_and_type_index }) => {
                assert_eq!(*bootstrap_index, 0);
                assert_eq!(*name_and_type_index, 31);
            }
            _ => panic!("#30 is not an InvokeDynamic"),
        }
        match cf.constant(48) {
            Some(ConstantPoolEntry::InvokeDynamic { bootstrap_index, .. }) => {
                assert_eq!(*bootstrap_index, 1);
            }
            _ => panic!("#48 is not an InvokeDynamic"),
        }

        match cf.constant(40) {
            Some(ConstantPoolEntry::StringIndex(i)) => assert_eq!(utf8_at(cf, *i), "a"),
            _ => panic!("#40 is not a String"),
        }

        match cf.constant(42) {
            Some(ConstantPoolEntry::InterfaceMethodRef { class_index, .. }) => {
                assert_eq!(class_name_at(cf, *class_index), "java/util/List");
            }
            _ => panic!("#42 is not an InterfaceMethodref"),
        }

        // Long at #61 takes #62 too, so #63 is the next real entry.
        assert!(matches!(cf.constant(61), Some(ConstantPoolEntry::Long(3))));
        assert!(cf.constant(62).is_none());
        assert!(matches!(cf.constant(63), Some(ConstantPoolEntry::InterfaceMethodRef { .. })));

        assert!(matches!(cf.constant(128), Some(ConstantPoolEntry::Integer(42))));
        assert!(matches!(cf.constant(131), Some(ConstantPoolEntry::Long(1234567890123))));
        assert_eq!(utf8_at(cf, 133), "FLOAT_CONST");
        match cf.constant(135) {
            Some(ConstantPoolEntry::Float(f)) => assert_eq!(*f, 3.5f32),
            _ => panic!("#135 is not a Float"),
        }
        match cf.constant(138) {
            Some(ConstantPoolEntry::Double(d)) => assert_eq!(*d, 2.718281828459045),
            _ => panic!("#138 is not a Double"),
        }
        assert_eq!(utf8_at(cf, 140), "STRING_CONST");
        assert!(matches!(cf.constant(145), Some(ConstantPoolEntry::Long(-9223372036854775807))));
        assert_eq!(utf8_at(cf, 147), "Signature");

        match cf.constant(215) {
            Some(ConstantPoolEntry::MethodHandle { ref_kind, ref_index }) => {
                assert_eq!(*ref_kind, MethodHandleKind::InvokeStatic);
                assert_eq!(*ref_index, 216);
            }
            _ => panic!("#215 is not a MethodHandle"),
        }
        assert!(matches!(cf.constant(230), Some(ConstantPoolEntry::MethodTypeIndex(56))));
        match cf.constant(231) {
            Some(ConstantPoolEntry::MethodHandle { ref_kind, ref_index }) => {
                assert_eq!(*ref_kind, MethodHandleKind::InvokeVirtual);
                assert_eq!(*ref_index, 232);
            }
            _ => panic!("#231 is not a MethodHandle"),
        }

        // "sum = \u{1}" is the string concat recipe; \u{1} is the raw byte 0x01.
        assert_eq!(utf8_at(cf, 222), "sum = \u{1}");
        assert_eq!(utf8_at(cf, 241), "Lookup");
    });
}

#[test]
fn hello_fields() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        let f = find_field(cf, "INT_CONST");
        assert_eq!(f.access_flags, FF::PUBLIC | FF::STATIC | FF::FINAL);
        assert_utf8_eq(f.descriptor, "I");
        assert_eq!(f.attributes.len(), 1);
        match &f.attributes[0] {
            AttributeInfo::ConstantValueIndex { value } => assert_eq!(*value, 128),
            _ => panic!("INT_CONST has no ConstantValue"),
        }

        let f = find_field(cf, "LONG_CONST");
        let value = require_attr!(f.attributes, AttributeInfo::ConstantValueIndex { value } => value);
        assert_eq!(*value, 131);

        let f = find_field(cf, "STRING_CONST");
        let value = require_attr!(f.attributes, AttributeInfo::ConstantValueIndex { value } => value);
        match cf.constant(*value) {
            Some(ConstantPoolEntry::StringIndex(i)) => {
                assert_eq!(utf8_at(cf, *i), "hello, class file")
            }
            _ => panic!("STRING_CONST's ConstantValue is not a String"),
        }

        let f = find_field(cf, "NEGATIVE_LONG");
        assert_eq!(f.access_flags, FF::PRIVATE | FF::STATIC | FF::FINAL);

        let f = find_field(cf, "names");
        assert_eq!(f.access_flags, FF::PRIVATE | FF::FINAL);
        assert_eq!(f.attributes.len(), 1);
        match &f.attributes[0] {
            AttributeInfo::Signature { value } => {
                assert_utf8_eq(*value, "Ljava/util/List<Ljava/lang/String;>;")
            }
            _ => panic!("names has no Signature"),
        }

        let f = find_field(cf, "counter");
        assert_eq!(f.access_flags, FF::PROTECTED | FF::VOLATILE);
        assert!(f.access_flags.contains(FF::VOLATILE));
        assert_eq!(f.attributes.len(), 0);

        let f = find_field(cf, "cache");
        assert!(f.access_flags.contains(FF::TRANSIENT));
    });
}

#[test]
fn hello_constructor_code() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        let m = find_method(cf, "<init>", Some("()V"));
        assert_eq!(m.access_flags, MF::PUBLIC);
        assert_eq!(m.attributes.len(), 1);
        let (max_stack, max_locals, code, exception_table, attributes) = require_attr!(
            m.attributes,
            AttributeInfo::Code { max_stack, max_locals, code, exception_table, attributes }
                => (max_stack, max_locals, code, exception_table, attributes)
        );
        assert_eq!(*max_stack, 3);
        assert_eq!(*max_locals, 1);
        assert_eq!(code.len(), 16);
        let expected: &[u8] = &[
            0x2a, 0xb7, 0x00, 0x01, 0x2a, 0xbb, 0x00, 0x07, 0x59, 0xb7, 0x00, 0x09, 0xb5, 0x00,
            0x0a, 0xb1,
        ];
        assert_eq!(*code, expected);
        assert_eq!(exception_table.len(), 0);

        assert_eq!(attributes.len(), 2);
        let lines =
            require_attr!(attributes, AttributeInfo::LineNumberTable { entries } => entries);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].line_number, 20);
        assert_eq!(lines[1].start_pc, 4);
        assert_eq!(lines[1].line_number, 16);

        let locals =
            require_attr!(attributes, AttributeInfo::LocalVariableTable { entries } => entries);
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].length, 16);
        assert_utf8_eq(locals[0].name, "this");
        assert_utf8_eq(locals[0].descriptor, "Lfixtures/Hello;");
    });
}

#[test]
fn hello_main_exceptions_and_parameters() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        let m = find_method(cf, "main", Some("([Ljava/lang/String;)V"));
        assert_eq!(m.access_flags, MF::PUBLIC | MF::STATIC);
        assert_eq!(m.attributes.len(), 3);

        let thrown = require_attr!(m.attributes, AttributeInfo::Exceptions { entries } => entries);
        assert_eq!(thrown.len(), 2);
        assert_utf8_eq(thrown[0], "java/io/IOException");
        assert_utf8_eq(thrown[1], "java/lang/InterruptedException");

        let params = require_attr!(m.attributes, AttributeInfo::MethodParameters { value } => value);
        assert_eq!(params.len(), 1);
        assert_utf8_eq(params[0].name.expect("a parameter name"), "args");
        assert_eq!(params[0].access_flags, PF::empty());

        let code = require_attr!(m.attributes, AttributeInfo::Code { code, .. } => code);
        assert_eq!(code.len(), 30);
        assert_eq!(code[21], 0xba); // invokedynamic
    });
}

#[test]
fn hello_sum_stack_map_table() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        let m = find_method(cf, "sum", Some("(I)I"));
        let (max_stack, max_locals, code, attributes) = require_attr!(
            m.attributes,
            AttributeInfo::Code { max_stack, max_locals, code, attributes, .. }
                => (max_stack, max_locals, code, attributes)
        );
        assert_eq!(*max_stack, 4);
        assert_eq!(*max_locals, 6);
        assert_eq!(code.len(), 44);
        assert_eq!(attributes.len(), 3);

        let locals =
            require_attr!(attributes, AttributeInfo::LocalVariableTable { entries } => entries);
        assert_eq!(locals.len(), 5);
        let wide = &locals[0];
        assert_eq!(wide.start_pc, 30);
        assert_eq!(wide.length, 6);
        assert_eq!(wide.slot, 4);
        assert_utf8_eq(wide.name, "wide");
        assert_utf8_eq(wide.descriptor, "J");

        let frames = require_attr!(attributes, AttributeInfo::StackMapTable { entries } => entries);
        assert_eq!(frames.len(), 4);
        match &frames[0] {
            StackMapFrame::AppendFrame { frame_type, offset_delta, locals } => {
                assert_eq!(*frame_type, 253);
                assert_eq!(*offset_delta, 4);
                assert_eq!(locals.len(), 2);
                assert!(matches!(locals[0], VerificationTypeInfo::IntegerVariableInfo));
                assert!(matches!(locals[1], VerificationTypeInfo::IntegerVariableInfo));
            }
            _ => panic!("frame 0 is not an append frame"),
        }
        match &frames[1] {
            StackMapFrame::SameFrame { frame_type } => assert_eq!(*frame_type, 17),
            _ => panic!("frame 1 is not a same frame"),
        }
        match &frames[2] {
            StackMapFrame::SameFrame { frame_type } => assert_eq!(*frame_type, 13),
            _ => panic!("frame 2 is not a same frame"),
        }
        match &frames[3] {
            StackMapFrame::ChopFrame { frame_type, offset_delta } => {
                assert_eq!(*frame_type, 250);
                assert_eq!(*offset_delta, 5);
            }
            _ => panic!("frame 3 is not a chop frame"),
        }

        let params = require_attr!(m.attributes, AttributeInfo::MethodParameters { value } => value);
        assert_utf8_eq(params[0].name.expect("a parameter name"), "n");
    });
}

#[test]
fn hello_generic_method_signature_and_type_tables() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        let m = find_method(cf, "max", Some("(Ljava/util/List;)Ljava/lang/Comparable;"));
        assert_eq!(m.access_flags, MF::PUBLIC | MF::SYNCHRONIZED);
        assert_eq!(m.attributes.len(), 3);
        let signature = require_attr!(m.attributes, AttributeInfo::Signature { value } => value);
        assert_utf8_eq(*signature, "<T::Ljava/lang/Comparable<TT;>;>(Ljava/util/List<TT;>;)TT;");

        let attributes = require_attr!(m.attributes, AttributeInfo::Code { attributes, .. } => attributes);
        assert_eq!(attributes.len(), 4);
        let types =
            require_attr!(attributes, AttributeInfo::LocalVariableTypeTable { entries } => entries);
        assert_eq!(types.len(), 3);
        let items = &types[1];
        assert_utf8_eq(items.name, "items");
        assert_utf8_eq(items.signature, "Ljava/util/List<TT;>;");
        assert_eq!(items.slot, 1);

        let frames = require_attr!(attributes, AttributeInfo::StackMapTable { entries } => entries);
        match &frames[0] {
            StackMapFrame::AppendFrame { offset_delta, locals, .. } => {
                assert_eq!(*offset_delta, 9);
                assert_eq!(object_class(cf, &locals[0]), "java/lang/Comparable");
                assert_eq!(object_class(cf, &locals[1]), "java/util/Iterator");
            }
            _ => panic!("frame 0 is not an append frame"),
        }
        match &frames[1] {
            StackMapFrame::AppendFrame { offset_delta, .. } => assert_eq!(*offset_delta, 34),
            _ => panic!("frame 1 is not an append frame"),
        }
    });
}

#[test]
fn hello_try_catch_exception_table() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        let m = find_method(cf, "tryCatch", None);
        let (exception_table, attributes) = require_attr!(
            m.attributes,
            AttributeInfo::Code { exception_table, attributes, .. } => (exception_table, attributes)
        );
        assert_eq!(exception_table.len(), 5);
        let t = &exception_table[..];
        assert_eq!(t[0].start_pc, 0);
        assert_eq!(t[0].end_pc, 8);
        assert_eq!(t[0].handler_pc, 20);
        assert_utf8_eq(t[0].catch_type.expect("a catch type"), "java/lang/NumberFormatException");
        assert_utf8_eq(t[1].catch_type.expect("a catch type"), "java/lang/NullPointerException");
        assert_eq!(t[2].handler_pc, 36);
        assert!(t[2].catch_type.is_none()); // "any", the finally handler
        assert_eq!(t[3].start_pc, 20);
        assert_eq!(t[3].end_pc, 24);
        assert_eq!(t[4].start_pc, 36);
        assert_eq!(t[4].end_pc, 38);
        assert_eq!(t[4].handler_pc, 36);

        let frames = require_attr!(attributes, AttributeInfo::StackMapTable { entries } => entries);
        assert_eq!(frames.len(), 2);
        match &frames[0] {
            StackMapFrame::SameLocals1StackItemFrame { frame_type, stack } => {
                assert_eq!(*frame_type, 84);
                assert_eq!(object_class(cf, stack), "java/lang/RuntimeException");
            }
            _ => panic!("frame 0 is not a same_locals_1_stack_item frame"),
        }
        match &frames[1] {
            StackMapFrame::SameLocals1StackItemFrame { frame_type, stack } => {
                assert_eq!(*frame_type, 79);
                assert_eq!(object_class(cf, stack), "java/lang/Throwable");
            }
            _ => panic!("frame 1 is not a same_locals_1_stack_item frame"),
        }
    });
}

#[test]
fn hello_native_deprecated_method() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        let m = find_method(cf, "nativeMethod", Some("(DJ)V"));
        assert_eq!(m.access_flags, MF::PUBLIC | MF::STATIC | MF::NATIVE);
        assert_eq!(m.attributes.len(), 3);
        assert!(find_attr!(m.attributes, AttributeInfo::Code { .. } => ()).is_none());
        require_attr!(m.attributes, AttributeInfo::Deprecated => ());

        let params = require_attr!(m.attributes, AttributeInfo::MethodParameters { value } => value);
        assert_eq!(params.len(), 2);
        assert_utf8_eq(params[1].name.expect("a parameter name"), "l");

        let annotations =
            require_attr!(m.attributes, AttributeInfo::RuntimeVisibleAnnotations { value } => value);
        assert_eq!(annotations.len(), 1);
        assert_utf8_eq(annotations[0].type_descriptor, "Ljava/lang/Deprecated;");
    });
}

#[test]
fn hello_varargs_switch_and_synthetic_lambda() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        let v = find_method(cf, "varargs", Some("([I)I"));
        assert!(v.access_flags.contains(MF::VARARGS));

        let sw = find_method(cf, "switchOnString", None);
        let (code, attributes) = require_attr!(
            sw.attributes,
            AttributeInfo::Code { code, attributes, .. } => (code, attributes)
        );
        assert_eq!(code.len(), 97);
        assert_eq!(code[8], 0xab); // lookupswitch

        let frames = require_attr!(attributes, AttributeInfo::StackMapTable { entries } => entries);
        assert_eq!(frames.len(), 6);
        match &frames[0] {
            StackMapFrame::AppendFrame { offset_delta, .. } => assert_eq!(*offset_delta, 36),
            _ => panic!("frame 0 is not an append frame"),
        }
        match &frames[5] {
            StackMapFrame::SameFrame { frame_type } => assert_eq!(*frame_type, 2),
            _ => panic!("frame 5 is not a same frame"),
        }

        let lambda = find_method(cf, "lambda$run$0", Some("()I"));
        assert_eq!(lambda.access_flags, MF::PRIVATE | MF::SYNTHETIC);
    });
}

#[test]
fn hello_class_attributes() {
    let bytes = fixture("fixtures/Hello.class");
    assert_parses(&bytes, |cf| {
        let source = require_attr!(cf.attributes, AttributeInfo::SourceFileIndex { value } => value);
        assert_utf8_eq(*source, "Hello.java");

        let members = require_attr!(cf.attributes, AttributeInfo::NestMembers { value } => value);
        assert_eq!(members.len(), 3);
        assert_utf8_eq(members[0], "fixtures/Hello$Nested");
        assert_utf8_eq(members[2], "fixtures/Hello$1");

        let bootstrap = require_attr!(cf.attributes, AttributeInfo::BootstrapMethods { value } => value);
        assert_eq!(bootstrap.len(), 2);
        let b0 = &bootstrap[0];
        let b1 = &bootstrap[1];
        assert_eq!(b0.bootstrap_method_ref, 215);
        assert_eq!(b0.bootstrap_arguments, vec![221]);
        assert_eq!(b1.bootstrap_method_ref, 223);
        assert_eq!(b1.bootstrap_arguments, vec![230, 231, 230]);

        let inner = require_attr!(cf.attributes, AttributeInfo::InnerClasses { value } => value);
        assert_eq!(inner.len(), 4);
        let anon = &inner[0]; // #98
        assert_utf8_eq(anon.inner_class, "fixtures/Hello$1");
        assert!(anon.outer_class.is_none());
        assert!(anon.inner_name.is_none());
        let nested = &inner[1]; // #210 of #11
        assert_utf8_eq(nested.inner_class, "fixtures/Hello$Nested");
        assert_utf8_eq(nested.outer_class.expect("an outer class"), "fixtures/Hello");
        assert_utf8_eq(nested.inner_name.expect("an inner name"), "Nested");
        assert_eq!(nested.access_flags, IF::STATIC);
        let lookup = &inner[3];
        assert_eq!(lookup.access_flags, IF::PUBLIC | IF::STATIC | IF::FINAL);
    });
}

#[test]
fn hello_inner_and_anonymous_classes() {
    let inner_bytes = fixture("fixtures/Hello$Inner.class");
    let anon_bytes = fixture("fixtures/Hello$1.class");
    let nested_bytes = fixture("fixtures/Hello$Nested.class");

    assert_parses(&inner_bytes, |inner| {
        assert_utf8_eq(inner.this_class, "fixtures/Hello$Inner");
        let host = require_attr!(inner.attributes, AttributeInfo::NestHostClass { value } => value);
        assert_utf8_eq(*host, "fixtures/Hello");
        let outer_this = find_field(inner, "this$0");
        assert!(outer_this.access_flags.contains(FF::SYNTHETIC));
    });

    assert_parses(&anon_bytes, |anon| {
        let (enclosing_class, enclosing_method) = require_attr!(
            anon.attributes,
            AttributeInfo::EnclosingMethod { enclosing_class, enclosing_method }
                => (enclosing_class, enclosing_method)
        );
        assert_utf8_eq(*enclosing_class, "fixtures/Hello");
        let method = enclosing_method.expect("an enclosing method");
        match anon.constant(method) {
            Some(ConstantPoolEntry::NameAndType { name_index, .. }) => {
                assert_eq!(utf8_at(anon, *name_index), "anonymous")
            }
            _ => panic!("the enclosing method is not a NameAndType"),
        }
        assert_eq!(anon.access_flags, CF::SUPER);
    });

    assert_parses(&nested_bytes, |nested| {
        let secret = find_method(nested, "secret", Some("()I"));
        assert_eq!(secret.access_flags, MF::PRIVATE);
    });
}

// ---------------------------------------------------------------------------
// Records, sealed types, enums
// ---------------------------------------------------------------------------

#[test]
fn point_record() {
    let bytes = fixture("fixtures/Point.class");
    assert_parses(&bytes, |cf| {
        assert_eq!(constant_pool_count(cf), 81);
        assert_eq!(cf.access_flags, CF::PUBLIC | CF::FINAL | CF::SUPER);
        assert_utf8_eq(cf.super_class.expect("a super class"), "java/lang/Record");
        assert_eq!(cf.fields.len(), 3);
        assert_eq!(cf.methods.len(), 7);
        assert_eq!(cf.attributes.len(), 4);

        let record = require_attr!(cf.attributes, AttributeInfo::Record { value } => value);
        let c = &record.record_components;
        assert_eq!(c.len(), 3);
        assert_utf8_eq(c[0].name, "x");
        assert_utf8_eq(c[0].descriptor, "I");
        assert_eq!(c[0].attributes.len(), 0);
        assert_utf8_eq(c[1].name, "y");
        assert_utf8_eq(c[2].name, "tags");
        assert_utf8_eq(c[2].descriptor, "Ljava/util/List;");
        assert_eq!(c[2].attributes.len(), 1);
        match &c[2].attributes[0] {
            // #39 in the pool
            AttributeInfo::Signature { value } => {
                assert_utf8_eq(*value, "Ljava/util/List<Ljava/lang/String;>;")
            }
            _ => panic!("the tags component has no Signature"),
        }

        let bootstrap = require_attr!(cf.attributes, AttributeInfo::BootstrapMethods { value } => value);
        assert_eq!(bootstrap.len(), 1);
        let b = &bootstrap[0];
        assert_eq!(b.bootstrap_method_ref, 63);
        assert_eq!(b.bootstrap_arguments.len(), 5);
        assert_eq!(b.bootstrap_arguments[0], 15);
        assert_eq!(b.bootstrap_arguments[1], 70);
        match cf.constant(72) {
            Some(ConstantPoolEntry::MethodHandle { ref_kind, .. }) => {
                assert_eq!(*ref_kind, MethodHandleKind::GetField)
            }
            _ => panic!("#72 is not a MethodHandle"),
        }
        match cf.constant(70) {
            Some(ConstantPoolEntry::StringIndex(i)) => assert_eq!(utf8_at(cf, *i), "x;y;tags"),
            _ => panic!("#70 is not a String"),
        }
    });
}

#[test]
fn shape_sealed_interface() {
    let bytes = fixture("fixtures/Shape.class");
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.access_flags, CF::PUBLIC | CF::INTERFACE | CF::ABSTRACT);
        assert_eq!(cf.methods.len(), 4);
        assert_eq!(cf.attributes.len(), 5);

        let permitted = require_attr!(cf.attributes, AttributeInfo::PermittedSubclasses { value } => value);
        assert_eq!(permitted.len(), 2);
        assert_utf8_eq(permitted[0], "fixtures/Shape$Circle");
        assert_utf8_eq(permitted[1], "fixtures/Shape$Square");

        let area = find_method(cf, "area", Some("()D"));
        assert_eq!(area.access_flags, MF::PUBLIC | MF::ABSTRACT);
        assert_eq!(area.attributes.len(), 0);

        let describe = find_method(cf, "describe", None);
        assert_eq!(describe.access_flags, MF::PUBLIC);
        require_attr!(describe.attributes, AttributeInfo::Code { .. } => ());

        let unit = find_method(cf, "unit", None);
        assert_eq!(unit.access_flags, MF::PUBLIC | MF::STATIC);

        let square = find_method(cf, "square", Some("(D)D"));
        assert_eq!(square.access_flags, MF::PRIVATE | MF::STATIC);
    });
}

#[test]
fn color_enum() {
    let bytes = fixture("fixtures/Color.class");
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.access_flags, CF::PUBLIC | CF::FINAL | CF::SUPER | CF::ENUM);
        assert_utf8_eq(cf.super_class.expect("a super class"), "java/lang/Enum");
        assert_eq!(cf.fields.len(), 4);

        let red = find_field(cf, "RED");
        assert_eq!(red.access_flags, FF::PUBLIC | FF::STATIC | FF::FINAL | FF::ENUM);
        let values = find_field(cf, "$VALUES");
        assert!(values.access_flags.contains(FF::SYNTHETIC));
        find_method(cf, "<clinit>", Some("()V"));

        let signature = require_attr!(cf.attributes, AttributeInfo::Signature { value } => value);
        assert_utf8_eq(*signature, "Ljava/lang/Enum<Lfixtures/Color;>;");
    });
}

// ---------------------------------------------------------------------------
// Constants: wide entries and modified UTF-8
// ---------------------------------------------------------------------------

#[test]
fn constants_wide_values() {
    let bytes = fixture("fixtures/Constants.class");
    assert_parses(&bytes, |cf| {
        assert_eq!(constant_pool_count(cf), 71);
        assert_eq!(cf.fields.len(), 11);

        let long_at = |index: u16| match cf.constant(index) {
            Some(ConstantPoolEntry::Long(v)) => *v,
            _ => panic!("#{index} is not a Long"),
        };
        let double_at = |index: u16| match cf.constant(index) {
            Some(ConstantPoolEntry::Double(v)) => *v,
            _ => panic!("#{index} is not a Double"),
        };
        let float_at = |index: u16| match cf.constant(index) {
            Some(ConstantPoolEntry::Float(v)) => *v,
            _ => panic!("#{index} is not a Float"),
        };

        assert_eq!(long_at(19), 0x7FEDCBA987654321);
        assert_eq!(long_at(26), 0x0123456789ABCDEF);
        assert!(double_bits_eq(double_at(30), 0x0000000000000001));
        assert_eq!(long_at(33), i64::MIN);
        assert!(double_bits_eq(double_at(36), 0x7FEFFFFFFFFFFFFF));
        assert_eq!(long_at(39), i64::MAX);
        assert_eq!(double_at(42), f64::NEG_INFINITY);
        assert!(double_at(45).is_nan());
        assert!(float_bits_eq(float_at(49), 0x00000001));
        assert!(float_bits_eq(float_at(51), 0x80000000));
        assert!(matches!(cf.constant(54), Some(ConstantPoolEntry::Integer(i32::MIN))));

        let l1 = find_field(cf, "L1");
        let value = require_attr!(l1.attributes, AttributeInfo::ConstantValueIndex { value } => value);
        assert_eq!(*value, 33);
    });
}

#[test]
fn constants_modified_utf8_string() {
    let bytes = fixture("fixtures/Constants.class");
    assert_parses(&bytes, |cf| {
        let index = match cf.constant(57) {
            Some(ConstantPoolEntry::StringIndex(i)) => *i,
            _ => panic!("#57 is not a String"),
        };
        // The same bytes tests/java_utf.rs decodes at the encoding level: a
        // two-byte NUL and a surrogate pair, neither of which is standard UTF-8.
        let modified: &[u8] = &[
            b'h', 0xc3, 0xa9, b'l', b'l', b'o', b' ', 0xe2, 0x82, 0xac, b' ', 0xed, 0xa0, 0xbd,
            0xed, 0xb8, 0x80, b' ', 0xc0, 0x80, b' ', b'e', b'n', b'd',
        ];
        // The pool keeps the raw bytes, so this asserts the encoding survived
        // the parse rather than just that it decoded to the right characters.
        assert_utf8_bytes_eq(utf8_at(cf, index), modified);
        assert_eq!(utf8_at(cf, index).to_string_checked().unwrap(), "héllo € \u{1F600} \u{0} end");
    });
}

// ---------------------------------------------------------------------------
// Annotations
// ---------------------------------------------------------------------------

#[test]
fn anno_annotation_defaults() {
    let bytes = fixture("fixtures/Anno.class");
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.access_flags, CF::PUBLIC | CF::INTERFACE | CF::ABSTRACT | CF::ANNOTATION);
        assert_eq!(cf.methods.len(), 14);
        assert_utf8_eq(cf.interfaces[0], "java/lang/annotation/Annotation");

        // Defaults that are a single constant pool index.
        let simple: &[(&str, u8, u16)] = &[
            ("b", b'B', 10),
            ("c", b'C', 13),
            ("s", b'S', 16),
            ("i", b'I', 19),
            ("j", b'J', 22),
            ("f", b'F', 26),
            ("d", b'D', 29),
            ("z", b'Z', 10),
            ("str", b's', 35),
        ];
        for (method, tag, index) in simple {
            let m = find_method(cf, method, None);
            let value = require_attr!(m.attributes, AttributeInfo::AnnotationDefault { value } => value);
            assert_eq!(value.tag(), *tag, "{method} default tag");
            let actual = match value {
                ElementValue::Byte(i)
                | ElementValue::Char(i)
                | ElementValue::Double(i)
                | ElementValue::Float(i)
                | ElementValue::Int(i)
                | ElementValue::Long(i)
                | ElementValue::Short(i)
                | ElementValue::Boolean(i)
                | ElementValue::String(i) => *i,
                _ => panic!("{method} default is not a constant index"),
            };
            assert_eq!(actual, *index, "{method} default index");
        }

        // `type` is a class default; the parser resolves #42 to its name.
        let m = find_method(cf, "type", None);
        let value = require_attr!(m.attributes, AttributeInfo::AnnotationDefault { value } => value);
        match value {
            ElementValue::Class(name) => assert_utf8_eq(*name, "Ljava/lang/Object;"),
            _ => panic!("type's default is not a class"),
        }

        let color = find_method(cf, "color", None);
        let value = require_attr!(color.attributes, AttributeInfo::AnnotationDefault { value } => value);
        match value {
            // #38 and #39 in the pool
            ElementValue::EnumConstant { type_name, const_name } => {
                assert_utf8_eq(*type_name, "Lfixtures/Color;");
                assert_utf8_eq(*const_name, "GREEN");
            }
            _ => panic!("color's default is not an enum constant"),
        }

        let nested = find_method(cf, "nested", None);
        let value = require_attr!(nested.attributes, AttributeInfo::AnnotationDefault { value } => value);
        match value {
            // #47 in the pool
            ElementValue::Annotation(a) => {
                assert_utf8_eq(a.type_descriptor, "Ljava/lang/annotation/Retention;");
                assert_eq!(a.element_value_pairs.len(), 1);
                assert_eq!(a.element_value_pairs[0].value.tag(), b'e');
            }
            _ => panic!("nested's default is not an annotation"),
        }

        let ints = find_method(cf, "ints", None);
        let value = require_attr!(ints.attributes, AttributeInfo::AnnotationDefault { value } => value);
        match value {
            ElementValue::Array(values) => {
                assert_eq!(values.len(), 3);
                assert!(matches!(values[1], ElementValue::Int(16)));
            }
            _ => panic!("ints' default is not an array"),
        }

        let empty = find_method(cf, "empty", None);
        let value = require_attr!(empty.attributes, AttributeInfo::AnnotationDefault { value } => value);
        match value {
            ElementValue::Array(values) => assert_eq!(values.len(), 0),
            _ => panic!("empty's default is not an array"),
        }

        let annotations =
            require_attr!(cf.attributes, AttributeInfo::RuntimeVisibleAnnotations { value } => value);
        assert_eq!(annotations.len(), 2);
    });
}

#[test]
fn annotated_class_annotations() {
    let bytes = fixture("fixtures/Annotated.class");
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.attributes.len(), 5);

        let visible =
            require_attr!(cf.attributes, AttributeInfo::RuntimeVisibleAnnotations { value } => value);
        assert_eq!(visible.len(), 1);
        let an = &visible[0];
        assert_utf8_eq(an.type_descriptor, "Lfixtures/Anno;");
        assert_eq!(an.element_value_pairs.len(), 5);
        let p = &an.element_value_pairs;
        assert_utf8_eq(p[0].element_name, "i");
        assert_eq!(p[0].value.tag(), b'I');
        match &p[0].value {
            ElementValue::Int(i) => {
                assert!(matches!(cf.constant(*i), Some(ConstantPoolEntry::Integer(99))))
            }
            _ => panic!("i is not an int"),
        }
        assert_eq!(p[1].value.tag(), b's');
        match &p[1].value {
            ElementValue::String(i) => assert_eq!(utf8_at(cf, *i), "on class"),
            _ => panic!("str is not a string"),
        }
        assert_eq!(p[2].value.tag(), b'e');
        match &p[2].value {
            ElementValue::EnumConstant { type_name, const_name } => {
                assert_utf8_eq(*type_name, "Lfixtures/Color;");
                assert_utf8_eq(*const_name, "BLUE");
            }
            _ => panic!("color is not an enum constant"),
        }
        assert_eq!(p[3].value.tag(), b'[');
        match &p[3].value {
            ElementValue::Array(values) => assert_eq!(values.len(), 1),
            _ => panic!("ints is not an array"),
        }
        assert_eq!(p[4].value.tag(), b'c');
        match &p[4].value {
            ElementValue::Class(name) => assert_utf8_eq(*name, "Ljava/lang/String;"),
            _ => panic!("type is not a class"),
        }

        let invisible =
            require_attr!(cf.attributes, AttributeInfo::RuntimeInvisibleAnnotations { value } => value);
        assert_eq!(invisible.len(), 1);
        assert_utf8_eq(invisible[0].type_descriptor, "Lfixtures/Annotated$Invisible;");
    });
}

#[test]
fn annotated_member_and_parameter_annotations() {
    let bytes = fixture("fixtures/Annotated.class");
    assert_parses(&bytes, |cf| {
        let field = find_field(cf, "field");
        assert_eq!(field.attributes.len(), 2);
        let visible =
            require_attr!(field.attributes, AttributeInfo::RuntimeVisibleAnnotations { value } => value);
        assert_eq!(visible[0].element_value_pairs[0].value.tag(), b'Z');

        let typed = find_field(cf, "typed");
        assert_eq!(typed.attributes.len(), 3);
        assert_eq!(count_unknown(&typed.attributes), 0);

        let m = find_method(cf, "method", Some("(ILjava/lang/String;Ljava/lang/Object;)V"));
        assert_eq!(m.attributes.len(), 8);
        assert_eq!(count_unknown(&m.attributes), 0);

        let params = require_attr!(m.attributes, AttributeInfo::MethodParameters { value } => value);
        assert_eq!(params.len(), 3);
        assert_eq!(params[2].access_flags, PF::FINAL);
        assert!(params[2].access_flags.contains(PF::FINAL));

        let visible_params = require_attr!(
            m.attributes,
            AttributeInfo::RuntimeVisibleParameterAnnotations { value } => value
        );
        assert_eq!(visible_params.len(), 3);
        assert_eq!(visible_params[0].annotations.len(), 1);
        let pair = &visible_params[0].annotations[0].element_value_pairs[0];
        assert_eq!(pair.value.tag(), b'J');
        match &pair.value {
            ElementValue::Long(i) => {
                assert!(matches!(cf.constant(*i), Some(ConstantPoolEntry::Long(10))))
            }
            _ => panic!("j is not a long"),
        }
        assert_eq!(visible_params[1].annotations.len(), 0);
        assert_eq!(visible_params[2].annotations.len(), 0);

        let invisible_params = require_attr!(
            m.attributes,
            AttributeInfo::RuntimeInvisibleParameterAnnotations { value } => value
        );
        assert_eq!(invisible_params.len(), 3);
        assert_eq!(invisible_params[1].annotations.len(), 1);

        let attributes = require_attr!(m.attributes, AttributeInfo::Code { attributes, .. } => attributes);
        assert_eq!(attributes.len(), 4);
        assert_eq!(count_unknown(attributes), 0);
    });
}

#[test]
fn annotation_interfaces() {
    let bytes = fixture("fixtures/Annotated$TypeUse.class");
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.access_flags, CF::INTERFACE | CF::ABSTRACT | CF::ANNOTATION);
        assert_eq!(cf.attributes.len(), 4);
        require_attr!(cf.attributes, AttributeInfo::NestHostClass { .. } => ());
        require_attr!(cf.attributes, AttributeInfo::InnerClasses { .. } => ());
        let visible =
            require_attr!(cf.attributes, AttributeInfo::RuntimeVisibleAnnotations { value } => value);
        assert_eq!(visible.len(), 2);
    });
}

// ---------------------------------------------------------------------------
// module-info and older versions
// ---------------------------------------------------------------------------

#[test]
fn module_info_fixture() {
    let bytes = fixture("module-info.class");
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.access_flags, CF::MODULE);
        assert_eq!(constant_pool_count(cf), 29);
        assert_utf8_eq(cf.this_class, "module-info"); // #2
        assert!(cf.super_class.is_none()); // super_class #0
        assert_eq!(cf.attributes.len(), 4);

        let module = require_attr!(cf.attributes, AttributeInfo::Module { value } => value);
        assert_utf8_eq(module.module_name, "test.fixtures"); // #5
        assert_eq!(module.module_flags, MdF::empty());
        assert!(module.module_version.is_none());

        assert_eq!(module.requires.len(), 4);
        let r = &module.requires;
        assert_utf8_eq(r[0].requires, "java.base"); // #13
        assert_eq!(r[0].requires_flags, RF::MANDATED);
        assert_utf8_eq(r[1].requires, "java.logging"); // #16
        assert_eq!(r[1].requires_flags, RF::empty());
        assert_utf8_eq(r[2].requires, "java.sql"); // #18
        assert_eq!(r[2].requires_flags, RF::TRANSITIVE);
        assert_utf8_eq(r[3].requires, "java.desktop"); // #20
        assert_eq!(r[3].requires_flags, RF::STATIC_PHASE);

        assert_eq!(module.exports.len(), 1);
        assert_utf8_eq(module.exports[0].exports, "fixtures"); // #9, a CONSTANT_Package
        assert_eq!(module.exports[0].exports_flags, EF::empty());
        assert_eq!(module.exports[0].exports_to.len(), 0);

        assert_eq!(module.opens.len(), 1);
        assert_utf8_eq(module.opens[0].opens, "fixtures/internal"); // #11
        assert_eq!(module.opens[0].opens_to.len(), 1);
        assert_utf8_eq(module.opens[0].opens_to[0], "java.base"); // #13

        assert_eq!(module.uses.len(), 1);
        assert_utf8_eq(module.uses[0], "java/lang/Runnable"); // #22
        assert_eq!(module.provides.len(), 1);
        assert_utf8_eq(module.provides[0].provides, "java/lang/Runnable"); // #22
        assert_eq!(module.provides[0].provides_with.len(), 1);
        assert_utf8_eq(module.provides[0].provides_with[0], "fixtures/internal/Task"); // #24

        let packages = require_attr!(cf.attributes, AttributeInfo::ModulePackages { value } => value);
        assert_eq!(packages.len(), 2);
        assert_utf8_eq(packages[0], "fixtures"); // #9
        assert_utf8_eq(packages[1], "fixtures/internal"); // #11

        let main = require_attr!(cf.attributes, AttributeInfo::ModuleMainClass { value } => value);
        assert_utf8_eq(*main, "fixtures/Hello"); // #7

        let source = require_attr!(cf.attributes, AttributeInfo::SourceFileIndex { value } => value);
        assert_utf8_eq(*source, "module-info.java");
    });
}

#[test]
fn legacy_java_7_and_8_classes() {
    let bytes7 = fixture("Legacy_release7.class");
    let bytes8 = fixture("Legacy_release8.class");

    assert_parses(&bytes8, |c8| assert_eq!(c8.major_version, 52));

    assert_parses(&bytes7, |c7| {
        assert_eq!(c7.major_version, 51);
        assert_eq!(constant_pool_count(c7), 29);
        assert_eq!(c7.fields.len(), 1);
        assert_eq!(c7.methods.len(), 4);
        assert_eq!(c7.attributes.len(), 1);
        assert!(matches!(c7.attributes[0], AttributeInfo::InnerClasses { .. }));

        let get = find_method(c7, "get", Some("()I"));
        let (exception_table, attributes) = require_attr!(
            get.attributes,
            AttributeInfo::Code { exception_table, attributes, .. } => (exception_table, attributes)
        );
        assert!(exception_table.len() >= 2);
        assert_eq!(attributes.len(), 1);
        assert!(matches!(attributes[0], AttributeInfo::StackMapTable { .. }));
    });
}

// ---------------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------------

/// Every `.class` file under `dir`, recursively, without following symlinks.
fn collect_class_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        let path = entry.path();
        if file_type.is_dir() {
            collect_class_files(&path, out);
        } else if file_type.is_file()
            && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("class"))
        {
            out.push(path);
        }
    }
}

#[test]
fn corpus_every_class_parses() {
    // Rust's test harness has no skip status, so with CLASSFILE_CORPUS unset
    // this returns having checked nothing (the C version reported SKIP here).
    let Ok(dir) = std::env::var("CLASSFILE_CORPUS") else { return };

    let mut paths = Vec::new();
    collect_class_files(Path::new(&dir), &mut paths);
    assert!(!paths.is_empty(), "no .class files under {dir}");

    let mut failures = Vec::new();
    for path in &paths {
        let Ok(bytes) = std::fs::read(path) else { continue };
        with_parsed(&bytes, |result| {
            if let Err(e) = result {
                if failures.len() < 25 {
                    failures.push(format!("    {}: {e}", path.display()));
                } else {
                    failures.push(String::new());
                }
            }
        });
    }
    let failed = failures.len();
    failures.retain(|f| !f.is_empty());
    assert!(
        failed == 0,
        "{failed} of {} class files failed to parse:\n{}",
        paths.len(),
        failures.join("\n")
    );
}
