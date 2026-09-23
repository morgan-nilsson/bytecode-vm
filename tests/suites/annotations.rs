//! Annotation attributes (JVMS 4.7.16 - 4.7.22): RuntimeVisible/Invisible
//! Annotations, ParameterAnnotations, TypeAnnotations and AnnotationDefault,
//! covering every element_value kind and every type annotation target.
//!
//! Ported from `c_backup/tests/test_classfile_annotations.c`. The C suite could
//! read an `attribute_info` on its own; here everything goes through
//! `ClassFile::parse`, so each case builds a class carrying the attribute.

use crate::common::*;

use bytecode_vm::parser::class_file::{
    Annotation, AttributeInfo, ClassFile, ClassParserError, ConstantPoolEntry, ElementValue,
    ParameterAnnotation, TypeAnnotation, TypeAnnotationTarget,
};

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// Where an attribute is attached. JVMS 4.7.20's table gives each type
/// annotation target_type exactly one of these as its home, and `ClassFile::parse`
/// rejects a target that turns up anywhere else.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Class,
    Field,
    Method,
    Code,
}

/// A class plus the constant pool entries the annotation bodies refer to. The
/// builder is cloned per case, so one fixture serves a whole loop.
struct Anno {
    cb: ClassBuilder,
    rva: u16,
    ria: u16,
    rvpa: u16,
    ripa: u16,
    rvta: u16,
    rita: u16,
    ad: u16,
    /// "LAnno;", the annotation type descriptor.
    ty: u16,
    /// "value", the element name.
    name: u16,
    i: u16,
    j: u16,
    f: u16,
    d: u16,
    /// "text", the Utf8 an 's' element_value points at.
    s: u16,
    enum_type: u16,
    enum_const: u16,
    class_info: u16,
    /// "Code", for the targets that are only legal inside a Code attribute.
    code: u16,
    /// "Exceptions", plus the two classes its table names, so a `throws` target
    /// has something to index into.
    exceptions: u16,
    exc_a: u16,
    exc_b: u16,
}

impl Anno {
    fn new() -> Self {
        let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
        Anno {
            rva: cb.pool.utf8("RuntimeVisibleAnnotations"),
            ria: cb.pool.utf8("RuntimeInvisibleAnnotations"),
            rvpa: cb.pool.utf8("RuntimeVisibleParameterAnnotations"),
            ripa: cb.pool.utf8("RuntimeInvisibleParameterAnnotations"),
            rvta: cb.pool.utf8("RuntimeVisibleTypeAnnotations"),
            rita: cb.pool.utf8("RuntimeInvisibleTypeAnnotations"),
            ad: cb.pool.utf8("AnnotationDefault"),
            ty: cb.pool.utf8("LAnno;"),
            name: cb.pool.utf8("value"),
            i: cb.pool.integer(5),
            j: cb.pool.long(6),
            f: cb.pool.float_bits(0x4048_f5c3),
            d: cb.pool.double_bits(0x4009_21fb_5444_2d18),
            s: cb.pool.utf8("text"),
            enum_type: cb.pool.utf8("LColor;"),
            enum_const: cb.pool.utf8("RED"),
            class_info: cb.pool.utf8("Ljava/lang/String;"),
            code: cb.pool.utf8("Code"),
            exceptions: cb.pool.utf8("Exceptions"),
            exc_a: cb.pool.class("java/lang/Exception"),
            exc_b: cb.pool.class("java/io/IOException"),
            cb,
        }
    }

    /// The class with `body` attached as a class-level attribute.
    fn class_attr(&self, name_index: u16, body: &[u8]) -> Vec<u8> {
        let mut cb = self.cb.clone();
        cb.reserve_attributes(1);
        cb.attributes.attr_raw(name_index, body);
        cb.to_bytes()
    }

    /// The class with `body` attached to an abstract method `m`. Parameter
    /// annotations and AnnotationDefault only ever appear on a method.
    fn method_attr(&self, desc: &str, name_index: u16, body: &[u8]) -> Vec<u8> {
        let mut cb = self.cb.clone();
        cb.access_flags |= acc::ABSTRACT;
        cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", desc, 1);
        cb.methods.attr_raw(name_index, body);
        cb.to_bytes()
    }

    /// The class with `body` attached to a field `f`.
    fn field_attr(&self, name_index: u16, body: &[u8]) -> Vec<u8> {
        let mut cb = self.cb.clone();
        cb.add_field(acc::PUBLIC, "f", "I", 1);
        cb.fields.attr_raw(name_index, body);
        cb.to_bytes()
    }

    /// The class with `body` on a method that also declares three parameters
    /// and two checked exceptions, so the formal parameter and throws targets
    /// have real entries to point at.
    fn method_target_attr(&self, name_index: u16, body: &[u8]) -> Vec<u8> {
        let mut cb = self.cb.clone();
        cb.access_flags |= acc::ABSTRACT;
        cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", "(III)V", 2);
        cb.methods.attr_raw(name_index, body);
        cb.methods.attr_u2_table(self.exceptions, &[self.exc_a, self.exc_b]);
        cb.to_bytes()
    }

    /// The class with `body` inside the `Code` attribute of a method `m`. The
    /// code is padded to 64 bytes and given a four-entry exception table so
    /// every offset and table index the Code-only targets carry is in range.
    fn code_attr(&self, name_index: u16, body: &[u8]) -> Vec<u8> {
        let mut cb = self.cb.clone();
        cb.add_method(acc::PUBLIC | acc::STATIC, "m", "()V", 1);
        let mut code = vec![op::NOP; 63];
        code.push(op::RETURN);

        let attr = cb.methods.attr_begin(self.code);
        cb.methods.u2(1);
        cb.methods.u2(4); // max_locals, covering the localvar target's slots
        cb.methods.u4(code.len() as u32);
        cb.methods.raw(&code);
        cb.methods.u2(4); // exception_table_length
        for i in 0..4u16 {
            cb.methods.u2(i * 8); // start_pc
            cb.methods.u2(i * 8 + 8); // end_pc
            cb.methods.u2(40); // handler_pc
            cb.methods.u2(0); // catch_type: any
        }
        cb.methods.u2(1); // attributes_count
        cb.methods.attr_raw(name_index, body);
        cb.methods.attr_end(attr);
        cb.to_bytes()
    }

    /// The class with `body` attached wherever `place` says.
    fn attr_at(&self, place: Place, name_index: u16, body: &[u8]) -> Vec<u8> {
        match place {
            Place::Class => self.class_attr(name_index, body),
            Place::Field => self.field_attr(name_index, body),
            Place::Method => self.method_target_attr(name_index, body),
            Place::Code => self.code_attr(name_index, body),
        }
    }
}

// ---------------------------------------------------------------------------
// Body builders
// ---------------------------------------------------------------------------

/// `annotation { type_index; 0 pairs }`.
fn write_marker_annotation(b: &mut Bytes, type_index: u16) {
    b.u2(type_index);
    b.u2(0);
}

/// An annotations body holding one annotation with one pair "value" = `ev`.
fn single_pair_body(p: &Anno, ev: &Bytes) -> Bytes {
    let mut b = Bytes::new();
    b.u2(1); // num_annotations
    b.u2(p.ty);
    b.u2(1); // num_element_value_pairs
    b.u2(p.name);
    b.cat(ev);
    b
}

/// A one-byte-tag, one-index element_value.
fn const_element_value(tag: u8, index: u16) -> Bytes {
    let mut ev = Bytes::new();
    ev.u1(tag);
    ev.u2(index);
    ev
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

fn visible_annotations<'a, 'b>(cf: &'b ClassFile<'a>) -> &'b [Annotation<'a>] {
    for a in &cf.attributes {
        if let AttributeInfo::RuntimeVisibleAnnotations { value } = a {
            return value;
        }
    }
    panic!("the class has no RuntimeVisibleAnnotations attribute");
}

fn invisible_annotations<'a, 'b>(cf: &'b ClassFile<'a>) -> &'b [Annotation<'a>] {
    for a in &cf.attributes {
        if let AttributeInfo::RuntimeInvisibleAnnotations { value } = a {
            return value;
        }
    }
    panic!("the class has no RuntimeInvisibleAnnotations attribute");
}

fn parameter_annotations<'a, 'b>(
    cf: &'b ClassFile<'a>,
    visible: bool,
) -> &'b [ParameterAnnotation<'a>] {
    for a in &cf.methods[0].attributes {
        match a {
            AttributeInfo::RuntimeVisibleParameterAnnotations { value } if visible => return value,
            AttributeInfo::RuntimeInvisibleParameterAnnotations { value } if !visible => {
                return value
            }
            _ => {}
        }
    }
    panic!("the method has no parameter annotations attribute");
}

fn type_annotations_at<'a, 'b>(
    cf: &'b ClassFile<'a>,
    place: Place,
    visible: bool,
) -> &'b [TypeAnnotation<'a>] {
    let attributes = match place {
        Place::Class => &cf.attributes,
        Place::Field => &cf.fields[0].attributes,
        Place::Method => &cf.methods[0].attributes,
        Place::Code => {
            let code = cf.methods[0]
                .attributes
                .iter()
                .find(|a| matches!(a, AttributeInfo::Code { .. }));
            match code {
                Some(AttributeInfo::Code { attributes, .. }) => attributes,
                _ => panic!("the method has no Code attribute"),
            }
        }
    };
    for a in attributes {
        match a {
            AttributeInfo::RuntimeVisibleTypeAnnotations { value } if visible => return value,
            AttributeInfo::RuntimeInvisibleTypeAnnotations { value } if !visible => return value,
            _ => {}
        }
    }
    panic!("{place:?} has no type annotations attribute");
}

fn annotation_default<'a, 'b>(cf: &'b ClassFile<'a>) -> &'b ElementValue<'a> {
    for a in &cf.methods[0].attributes {
        if let AttributeInfo::AnnotationDefault { value } = a {
            return value;
        }
    }
    panic!("the method has no AnnotationDefault attribute");
}

/// The single pair's value from a class built with `single_pair_body`, checking
/// the pair's shape on the way through.
fn single_pair_value<'a, 'b>(cf: &'b ClassFile<'a>) -> &'b ElementValue<'a> {
    let annotations = visible_annotations(cf);
    assert_eq!(annotations.len(), 1);
    assert_utf8_eq(annotations[0].type_descriptor, "LAnno;");
    let pairs = &annotations[0].element_value_pairs;
    assert_eq!(pairs.len(), 1);
    assert_utf8_eq(pairs[0].element_name, "value");
    &pairs[0].value
}

/// The constant pool index inside any of the nine index-carrying element_value
/// kinds. The tag is kept separately, so this drops only the tag.
#[track_caller]
fn const_index(v: &ElementValue<'_>) -> u16 {
    match v {
        ElementValue::Byte(i)
        | ElementValue::Char(i)
        | ElementValue::Double(i)
        | ElementValue::Float(i)
        | ElementValue::Int(i)
        | ElementValue::Long(i)
        | ElementValue::Short(i)
        | ElementValue::Boolean(i)
        | ElementValue::String(i) => *i,
        other => panic!("expected a constant element_value, got tag '{}'", other.tag() as char),
    }
}

/// Every prefix of an attribute body must be rejected. The body is re-framed at
/// its cut length, so this exercises the structure rather than a length field
/// that disagrees with it. Every byte of the attribute is therefore present in
/// the file and it is the contents that fall short of `attribute_length`, which
/// is what makes this the length error rather than a truncated file.
#[track_caller]
fn assert_every_prefix_rejected(body: &Bytes, what: &str, build: impl Fn(&[u8]) -> Vec<u8>) {
    for len in 0..body.len() {
        let bytes = build(&body.prefix(len));
        let err = assert_rejected(&bytes);
        assert!(
            matches!(err, ClassParserError::ClassParseInvalidAttributeLength),
            "{what} cut to {len} of {} bytes gave {err:?}",
            body.len()
        );
    }
}

// ---------------------------------------------------------------------------
// RuntimeVisibleAnnotations / RuntimeInvisibleAnnotations (4.7.16, 4.7.17)
// ---------------------------------------------------------------------------

#[test]
fn runtime_visible_annotations_marker() {
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u2(1);
    write_marker_annotation(&mut b, p.ty);

    assert_parses(&p.class_attr(p.rva, &b.data), |cf| {
        let annotations = visible_annotations(cf);
        assert_eq!(annotations.len(), 1);
        assert_utf8_eq(annotations[0].type_descriptor, "LAnno;");
        assert!(annotations[0].element_value_pairs.is_empty());
    });
}

#[test]
fn runtime_invisible_annotations_multiple() {
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u2(3);
    write_marker_annotation(&mut b, p.ty);
    write_marker_annotation(&mut b, p.enum_type);
    write_marker_annotation(&mut b, p.class_info);

    assert_parses(&p.class_attr(p.ria, &b.data), |cf| {
        let annotations = invisible_annotations(cf);
        assert_eq!(annotations.len(), 3);
        assert_utf8_eq(annotations[0].type_descriptor, "LAnno;");
        assert_utf8_eq(annotations[1].type_descriptor, "LColor;");
        assert_utf8_eq(annotations[2].type_descriptor, "Ljava/lang/String;");
    });
}

#[test]
fn runtime_visible_annotations_empty() {
    // num_annotations of 0 is legal and must not be confused with a missing
    // attribute.
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u2(0);
    assert_parses(&p.class_attr(p.rva, &b.data), |cf| {
        assert!(visible_annotations(cf).is_empty());
    });
}

#[test]
fn element_value_const_kinds() {
    // The tag is not redundant with the constant it points at: 'B', 'C', 'I',
    // 'S' and 'Z' all index a CONSTANT_Integer but mean different types, so the
    // parser has to keep them apart.
    let p = Anno::new();
    let kinds = [
        (b'B', p.i),
        (b'C', p.i),
        (b'I', p.i),
        (b'S', p.i),
        (b'Z', p.i),
        (b'J', p.j),
        (b'F', p.f),
        (b'D', p.d),
        (b's', p.s),
    ];
    for (tag, index) in kinds {
        let ev = const_element_value(tag, index);
        let bytes = p.class_attr(p.rva, &single_pair_body(&p, &ev).data);
        assert_parses(&bytes, |cf| {
            let v = single_pair_value(cf);
            assert_eq!(
                v.tag(),
                tag,
                "element_value '{}' came back as '{}'",
                tag as char,
                v.tag() as char
            );
            assert_eq!(const_index(v), index);
        });
    }
}

#[test]
fn element_value_enum_constant() {
    let p = Anno::new();
    let mut ev = Bytes::new();
    ev.u1(b'e');
    ev.u2(p.enum_type);
    ev.u2(p.enum_const);

    let bytes = p.class_attr(p.rva, &single_pair_body(&p, &ev).data);
    assert_parses(&bytes, |cf| match single_pair_value(cf) {
        ElementValue::EnumConstant { type_name, const_name } => {
            assert_utf8_eq(*type_name, "LColor;");
            assert_utf8_eq(*const_name, "RED");
        }
        other => panic!("expected an enum constant, got tag '{}'", other.tag() as char),
    });
}

#[test]
fn element_value_class() {
    let p = Anno::new();
    let mut ev = Bytes::new();
    ev.u1(b'c');
    ev.u2(p.class_info);

    let bytes = p.class_attr(p.rva, &single_pair_body(&p, &ev).data);
    assert_parses(&bytes, |cf| match single_pair_value(cf) {
        ElementValue::Class(name) => assert_utf8_eq(*name, "Ljava/lang/String;"),
        other => panic!("expected a class element_value, got tag '{}'", other.tag() as char),
    });
}

#[test]
fn element_value_nested_annotation() {
    let p = Anno::new();
    let mut ev = Bytes::new();
    ev.u1(b'@');
    ev.u2(p.enum_type); // the nested annotation's own type
    ev.u2(1);
    ev.u2(p.name);
    ev.u1(b'I');
    ev.u2(p.i);

    let bytes = p.class_attr(p.rva, &single_pair_body(&p, &ev).data);
    assert_parses(&bytes, |cf| match single_pair_value(cf) {
        ElementValue::Annotation(nested) => {
            assert_utf8_eq(nested.type_descriptor, "LColor;");
            assert_eq!(nested.element_value_pairs.len(), 1);
            let inner = &nested.element_value_pairs[0];
            assert_utf8_eq(inner.element_name, "value");
            assert_eq!(inner.value.tag(), b'I');
            assert_eq!(const_index(&inner.value), p.i);
        }
        other => panic!("expected a nested annotation, got tag '{}'", other.tag() as char),
    });
}

#[test]
fn element_value_array() {
    // An array's elements need not share a tag; nothing in the file format ties
    // them together.
    let p = Anno::new();
    let mut ev = Bytes::new();
    ev.u1(b'[');
    ev.u2(3);
    ev.u1(b'I').u2(p.i);
    ev.u1(b's').u2(p.s);
    ev.u1(b'e').u2(p.enum_type).u2(p.enum_const);

    let bytes = p.class_attr(p.rva, &single_pair_body(&p, &ev).data);
    assert_parses(&bytes, |cf| match single_pair_value(cf) {
        ElementValue::Array(values) => {
            assert_eq!(values.len(), 3);
            assert_eq!(values[0].tag(), b'I');
            assert_eq!(values[1].tag(), b's');
            assert_eq!(const_index(&values[1]), p.s);
            match &values[2] {
                ElementValue::EnumConstant { const_name, .. } => {
                    assert_utf8_eq(*const_name, "RED")
                }
                other => panic!("expected an enum constant, got tag '{}'", other.tag() as char),
            }
        }
        other => panic!("expected an array, got tag '{}'", other.tag() as char),
    });
}

#[test]
fn element_value_empty_array() {
    let p = Anno::new();
    let mut ev = Bytes::new();
    ev.u1(b'[');
    ev.u2(0);

    let bytes = p.class_attr(p.rva, &single_pair_body(&p, &ev).data);
    assert_parses(&bytes, |cf| match single_pair_value(cf) {
        ElementValue::Array(values) => assert!(values.is_empty()),
        other => panic!("expected an array, got tag '{}'", other.tag() as char),
    });
}

#[test]
fn element_value_array_of_annotations_of_arrays() {
    // Three levels of mutual recursion between element_value and annotation.
    let p = Anno::new();
    let mut ev = Bytes::new();
    ev.u1(b'[');
    ev.u2(2);
    for _ in 0..2 {
        ev.u1(b'@');
        ev.u2(p.ty);
        ev.u2(1);
        ev.u2(p.name);
        ev.u1(b'[');
        ev.u2(2);
        ev.u1(b'J').u2(p.j);
        ev.u1(b'c').u2(p.class_info);
    }

    let bytes = p.class_attr(p.rva, &single_pair_body(&p, &ev).data);
    assert_parses(&bytes, |cf| {
        let outer = match single_pair_value(cf) {
            ElementValue::Array(values) => values,
            other => panic!("expected an array, got tag '{}'", other.tag() as char),
        };
        assert_eq!(outer.len(), 2);
        let second = match &outer[1] {
            ElementValue::Annotation(a) => a,
            other => panic!("expected an annotation, got tag '{}'", other.tag() as char),
        };
        match &second.element_value_pairs[0].value {
            ElementValue::Array(inner) => {
                assert_eq!(inner.len(), 2);
                assert_eq!(inner[0].tag(), b'J');
                match &inner[1] {
                    ElementValue::Class(name) => assert_utf8_eq(*name, "Ljava/lang/String;"),
                    other => panic!("expected a class, got tag '{}'", other.tag() as char),
                }
            }
            other => panic!("expected an array, got tag '{}'", other.tag() as char),
        }
    });
}

#[test]
fn element_value_deeply_nested_arrays_do_not_crash() {
    // Nothing bounds array nesting, so a hostile file can drive the parser as
    // deep as it likes; 200 levels must still come back as a value.
    let p = Anno::new();
    let mut ev = Bytes::new();
    for _ in 0..200 {
        ev.u1(b'[');
        ev.u2(1);
    }
    ev.u1(b'Z');
    ev.u2(p.i);

    let bytes = p.class_attr(p.rva, &single_pair_body(&p, &ev).data);
    assert_parses(&bytes, |cf| {
        let mut v = single_pair_value(cf);
        for depth in 0..200 {
            v = match v {
                ElementValue::Array(values) => {
                    assert_eq!(values.len(), 1, "array at depth {depth}");
                    &values[0]
                }
                other => panic!("expected an array at depth {depth}, got '{}'", other.tag() as char),
            };
        }
        assert_eq!(v.tag(), b'Z');
    });
}

#[test]
fn annotation_with_several_pairs() {
    // Pair order is the file's order; the names are arbitrary Utf8 entries.
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u2(1);
    b.u2(p.ty);
    b.u2(3);
    b.u2(p.name).u1(b'I').u2(p.i);
    b.u2(p.enum_const).u1(b'D').u2(p.d);
    b.u2(p.s).u1(b'c').u2(p.class_info);

    assert_parses(&p.class_attr(p.rva, &b.data), |cf| {
        let pairs = &visible_annotations(cf)[0].element_value_pairs;
        assert_eq!(pairs.len(), 3);
        assert_utf8_eq(pairs[0].element_name, "value");
        assert_utf8_eq(pairs[1].element_name, "RED");
        assert_eq!(pairs[1].value.tag(), b'D');
        assert_utf8_eq(pairs[2].element_name, "text");
        assert_eq!(pairs[2].value.tag(), b'c');
    });
}

#[test]
fn annotation_with_mixed_pairs() {
    // Ported from the C suite's `annotation_read_directly`: one annotation whose
    // pairs mix a scalar with an array, read through the class this time since
    // `Annotation::parse` is not a public entry point for tests.
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u2(1);
    b.u2(p.ty);
    b.u2(2);
    b.u2(p.name).u1(b'Z').u2(p.i);
    b.u2(p.s).u1(b'[').u2(1).u1(b's').u2(p.s);

    assert_parses(&p.class_attr(p.rva, &b.data), |cf| {
        let pairs = &visible_annotations(cf)[0].element_value_pairs;
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].value.tag(), b'Z');
        match &pairs[1].value {
            ElementValue::Array(values) => {
                assert_eq!(values.len(), 1);
                assert_eq!(const_index(&values[0]), p.s);
            }
            other => panic!("expected an array, got tag '{}'", other.tag() as char),
        }
    });
}

#[test]
fn element_value_invalid_tags_rejected() {
    // JVMS 4.7.16.1 fixes the tag alphabet; 'V' and 'L' are descriptor letters
    // that look plausible but are not element_value tags.
    let p = Anno::new();
    for bad in [b'X', b'V', b'L', b'a', b'(', 0x00, 0xff] {
        let ev = const_element_value(bad, p.i);
        let bytes = p.class_attr(p.rva, &single_pair_body(&p, &ev).data);
        assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidElementValueTag);
    }
}

#[test]
fn annotation_truncated_at_every_offset_rejected() {
    let p = Anno::new();
    let mut ev = Bytes::new();
    ev.u1(b'[');
    ev.u2(2);
    ev.u1(b'@').u2(p.ty).u2(1);
    ev.u2(p.name).u1(b'e').u2(p.enum_type).u2(p.enum_const);
    ev.u1(b'I').u2(p.i);
    let body = single_pair_body(&p, &ev);

    assert_every_prefix_rejected(&body, "annotation", |cut| p.class_attr(p.rva, cut));
}

// ---------------------------------------------------------------------------
// Parameter annotations (4.7.18, 4.7.19)
// ---------------------------------------------------------------------------

#[test]
fn runtime_visible_parameter_annotations() {
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u1(3); // num_parameters is a u1, unlike every other count here
    b.u2(1);
    write_marker_annotation(&mut b, p.ty);
    b.u2(0);
    b.u2(2);
    write_marker_annotation(&mut b, p.ty);
    write_marker_annotation(&mut b, p.enum_type);

    let bytes = p.method_attr("(II)V", p.rvpa, &b.data);
    assert_parses(&bytes, |cf| {
        let pa = parameter_annotations(cf, true);
        assert_eq!(pa.len(), 3);
        assert_eq!(pa[0].annotations.len(), 1);
        assert_utf8_eq(pa[0].annotations[0].type_descriptor, "LAnno;");
        assert!(pa[1].annotations.is_empty());
        assert_eq!(pa[2].annotations.len(), 2);
        assert_utf8_eq(pa[2].annotations[1].type_descriptor, "LColor;");
    });
}

#[test]
fn runtime_invisible_parameter_annotations() {
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u1(1);
    b.u2(1);
    b.u2(p.ty);
    b.u2(1);
    b.u2(p.name);
    b.u1(b'e').u2(p.enum_type).u2(p.enum_const);

    let bytes = p.method_attr("(II)V", p.ripa, &b.data);
    assert_parses(&bytes, |cf| {
        let pa = parameter_annotations(cf, false);
        assert_eq!(pa.len(), 1);
        let value = &pa[0].annotations[0].element_value_pairs[0].value;
        assert_eq!(value.tag(), b'e');
    });
}

#[test]
fn parameter_annotations_zero_parameters() {
    // A body of a single zero byte: the attribute is present but empty.
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u1(0);
    let bytes = p.method_attr("(II)V", p.rvpa, &b.data);
    assert_parses(&bytes, |cf| assert!(parameter_annotations(cf, true).is_empty()));
}

#[test]
fn parameter_annotations_255_parameters() {
    // 255 is the largest a u1 count can express, and the JVMS limit on method
    // parameters; it must not be mistaken for a sentinel.
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u1(255);
    for _ in 0..255 {
        b.u2(0);
    }
    let bytes = p.method_attr("(II)V", p.rvpa, &b.data);
    assert_parses(&bytes, |cf| {
        let pa = parameter_annotations(cf, true);
        assert_eq!(pa.len(), 255);
        assert!(pa.iter().all(|entry| entry.annotations.is_empty()));
    });
}

#[test]
fn parameter_annotations_truncated_at_every_offset_rejected() {
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u1(2);
    b.u2(1);
    write_marker_annotation(&mut b, p.ty);
    b.u2(0);

    assert_every_prefix_rejected(&b, "parameter annotations", |cut| {
        p.method_attr("(II)V", p.rvpa, cut)
    });
}

#[test]
fn parameter_annotations_count_mismatch_with_descriptor_is_allowed() {
    // javac emits fewer entries than the descriptor has parameters for some
    // synthetic parameters, so num_parameters is not checked against it.
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u1(1);
    b.u2(0);
    let bytes = p.method_attr("(III)V", p.rvpa, &b.data);
    assert_parses(&bytes, |cf| assert_eq!(parameter_annotations(cf, true).len(), 1));
}

// ---------------------------------------------------------------------------
// AnnotationDefault (4.7.22)
// ---------------------------------------------------------------------------

#[test]
fn annotation_default_int() {
    let p = Anno::new();
    let ev = const_element_value(b'I', p.i);
    let bytes = p.method_attr("()I", p.ad, &ev.data);
    assert_parses(&bytes, |cf| {
        let v = annotation_default(cf);
        assert_eq!(v.tag(), b'I');
        assert_eq!(const_index(v), p.i);
    });
}

#[test]
fn annotation_default_array_of_enums() {
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u1(b'[');
    b.u2(2);
    b.u1(b'e').u2(p.enum_type).u2(p.enum_const);
    b.u1(b'e').u2(p.enum_type).u2(p.name);

    let bytes = p.method_attr("()[LColor;", p.ad, &b.data);
    assert_parses(&bytes, |cf| match annotation_default(cf) {
        ElementValue::Array(values) => {
            assert_eq!(values.len(), 2);
            match &values[1] {
                ElementValue::EnumConstant { const_name, .. } => {
                    assert_utf8_eq(*const_name, "value")
                }
                other => panic!("expected an enum constant, got tag '{}'", other.tag() as char),
            }
        }
        other => panic!("expected an array, got tag '{}'", other.tag() as char),
    });
}

#[test]
fn annotation_default_nested_annotation() {
    let p = Anno::new();
    let mut b = Bytes::new();
    b.u1(b'@');
    write_marker_annotation(&mut b, p.ty);

    let bytes = p.method_attr("()LAnno;", p.ad, &b.data);
    assert_parses(&bytes, |cf| match annotation_default(cf) {
        ElementValue::Annotation(a) => assert_utf8_eq(a.type_descriptor, "LAnno;"),
        other => panic!("expected an annotation, got tag '{}'", other.tag() as char),
    });
}

#[test]
fn annotation_default_on_annotation_interface_method() {
    // The real shape: an @interface's element with a default, so the attribute
    // is reached through a method of an ACC_ANNOTATION interface.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    cb.access_flags = acc::PUBLIC | acc::INTERFACE | acc::ABSTRACT | acc::ANNOTATION;
    let annotation_interface = cb.pool.class("java/lang/annotation/Annotation");
    cb.add_interface(annotation_interface);
    let ad = cb.pool.utf8("AnnotationDefault");
    let v = cb.pool.integer(10);
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "value", "()I", 1);
    let body = cb.methods.attr_begin(ad);
    cb.methods.u1(b'I');
    cb.methods.u2(v);
    cb.methods.attr_end(body);

    assert_parses(&cb.to_bytes(), |cf| {
        let value = annotation_default(cf);
        assert_eq!(value.tag(), b'I');
        match cf.constant(const_index(value)) {
            Some(ConstantPoolEntry::Integer(n)) => assert_eq!(*n, 10),
            _ => panic!("the default's index does not point at a CONSTANT_Integer"),
        }
    });
}

// ---------------------------------------------------------------------------
// Type annotations (4.7.20, 4.7.21)
// ---------------------------------------------------------------------------

/// One row of the target_type table: the byte, its target_info bytes, and what
/// the parsed target must look like.
struct TypeTarget {
    target_type: u8,
    info: &'static [u8],
    what: &'static str,
    /// The only kind of attribute this target may appear in (JVMS 4.7.20.1).
    place: Place,
    check: fn(&TypeAnnotationTarget) -> bool,
}

/// Every target_type JVMS 4.7.20.1 defines, with a target_info whose fields are
/// all distinct so a parser that reads them in the wrong order is caught.
const TYPE_TARGETS: &[TypeTarget] = &[
    TypeTarget {
        target_type: 0x00,
        info: &[0],
        what: "class type parameter",
        place: Place::Class,
        check: |t| {
            matches!(t, TypeAnnotationTarget::TypeParameter { target_type: 0x00, type_parameter_index: 0 })
        },
    },
    TypeTarget {
        target_type: 0x01,
        info: &[1],
        what: "method type parameter",
        place: Place::Method,
        check: |t| {
            matches!(t, TypeAnnotationTarget::TypeParameter { target_type: 0x01, type_parameter_index: 1 })
        },
    },
    TypeTarget {
        target_type: 0x10,
        info: &[0xff, 0xff],
        what: "supertype: extends",
        place: Place::Class,
        check: |t| matches!(t, TypeAnnotationTarget::Supertype { supertype_index: 0xffff }),
    },
    TypeTarget {
        target_type: 0x10,
        info: &[0x00, 0x02],
        what: "supertype: implements #2",
        place: Place::Class,
        check: |t| matches!(t, TypeAnnotationTarget::Supertype { supertype_index: 2 }),
    },
    TypeTarget {
        target_type: 0x11,
        info: &[0, 1],
        what: "class type parameter bound",
        place: Place::Class,
        check: |t| {
            matches!(
                t,
                TypeAnnotationTarget::TypeParameterBound {
                    target_type: 0x11,
                    type_parameter_index: 0,
                    bound_index: 1
                }
            )
        },
    },
    TypeTarget {
        target_type: 0x12,
        info: &[1, 0],
        what: "method type parameter bound",
        place: Place::Method,
        check: |t| {
            matches!(
                t,
                TypeAnnotationTarget::TypeParameterBound {
                    target_type: 0x12,
                    type_parameter_index: 1,
                    bound_index: 0
                }
            )
        },
    },
    TypeTarget {
        target_type: 0x13,
        info: &[],
        what: "empty: field",
        place: Place::Field,
        check: |t| matches!(t, TypeAnnotationTarget::Empty { target_type: 0x13 }),
    },
    TypeTarget {
        target_type: 0x14,
        info: &[],
        what: "empty: return type",
        place: Place::Method,
        check: |t| matches!(t, TypeAnnotationTarget::Empty { target_type: 0x14 }),
    },
    TypeTarget {
        target_type: 0x15,
        info: &[],
        what: "empty: receiver",
        place: Place::Method,
        check: |t| matches!(t, TypeAnnotationTarget::Empty { target_type: 0x15 }),
    },
    TypeTarget {
        target_type: 0x16,
        info: &[2],
        what: "formal parameter",
        place: Place::Method,
        check: |t| matches!(t, TypeAnnotationTarget::FormalParameter { formal_parameter_index: 2 }),
    },
    TypeTarget {
        target_type: 0x17,
        info: &[0x00, 0x01],
        what: "throws",
        place: Place::Method,
        check: |t| matches!(t, TypeAnnotationTarget::Throws { throws_type_index: 1 }),
    },
    TypeTarget {
        target_type: 0x40,
        info: &[0x00, 0x02, 0, 0, 0, 5, 0, 1, 0, 3, 0, 9, 0, 2],
        what: "local variable, 2 entries",
        place: Place::Code,
        check: |t| match t {
            TypeAnnotationTarget::LocalVar { target_type: 0x40, table } => {
                table.len() == 2
                    && (table[0].start_pc, table[0].length, table[0].index) == (0, 5, 1)
                    && (table[1].start_pc, table[1].length, table[1].index) == (3, 9, 2)
            }
            _ => false,
        },
    },
    TypeTarget {
        target_type: 0x41,
        info: &[0x00, 0x01, 0, 0, 0, 5, 0, 1],
        what: "resource variable",
        place: Place::Code,
        check: |t| match t {
            TypeAnnotationTarget::LocalVar { target_type: 0x41, table } => {
                table.len() == 1 && (table[0].start_pc, table[0].length, table[0].index) == (0, 5, 1)
            }
            _ => false,
        },
    },
    TypeTarget {
        target_type: 0x42,
        info: &[0x00, 0x03],
        what: "catch",
        place: Place::Code,
        check: |t| matches!(t, TypeAnnotationTarget::Catch { exception_table_index: 3 }),
    },
    TypeTarget {
        target_type: 0x43,
        info: &[0x00, 0x10],
        what: "offset: instanceof",
        place: Place::Code,
        check: |t| matches!(t, TypeAnnotationTarget::Offset { target_type: 0x43, offset: 0x10 }),
    },
    TypeTarget {
        target_type: 0x44,
        info: &[0x00, 0x10],
        what: "offset: new",
        place: Place::Code,
        check: |t| matches!(t, TypeAnnotationTarget::Offset { target_type: 0x44, offset: 0x10 }),
    },
    TypeTarget {
        target_type: 0x45,
        info: &[0x00, 0x10],
        what: "offset: ::new",
        place: Place::Code,
        check: |t| matches!(t, TypeAnnotationTarget::Offset { target_type: 0x45, offset: 0x10 }),
    },
    TypeTarget {
        target_type: 0x46,
        info: &[0x00, 0x10],
        what: "offset: ::identifier",
        place: Place::Code,
        check: |t| matches!(t, TypeAnnotationTarget::Offset { target_type: 0x46, offset: 0x10 }),
    },
    TypeTarget {
        target_type: 0x47,
        info: &[0x00, 0x10, 0x01],
        what: "type argument: cast",
        place: Place::Code,
        check: |t| {
            matches!(
                t,
                TypeAnnotationTarget::TypeArgument {
                    target_type: 0x47,
                    offset: 0x10,
                    type_argument_index: 1
                }
            )
        },
    },
    TypeTarget {
        target_type: 0x48,
        info: &[0x00, 0x10, 0x00],
        what: "type argument: constructor invocation",
        place: Place::Code,
        check: |t| {
            matches!(
                t,
                TypeAnnotationTarget::TypeArgument {
                    target_type: 0x48,
                    offset: 0x10,
                    type_argument_index: 0
                }
            )
        },
    },
    TypeTarget {
        target_type: 0x49,
        info: &[0x00, 0x10, 0x02],
        what: "type argument: method invocation",
        place: Place::Code,
        check: |t| {
            matches!(
                t,
                TypeAnnotationTarget::TypeArgument {
                    target_type: 0x49,
                    offset: 0x10,
                    type_argument_index: 2
                }
            )
        },
    },
    TypeTarget {
        target_type: 0x4A,
        info: &[0x00, 0x10, 0x00],
        what: "type argument: ::new",
        place: Place::Code,
        check: |t| {
            matches!(
                t,
                TypeAnnotationTarget::TypeArgument {
                    target_type: 0x4A,
                    offset: 0x10,
                    type_argument_index: 0
                }
            )
        },
    },
    TypeTarget {
        target_type: 0x4B,
        info: &[0x00, 0x10, 0x01],
        what: "type argument: ::identifier",
        place: Place::Code,
        check: |t| {
            matches!(
                t,
                TypeAnnotationTarget::TypeArgument {
                    target_type: 0x4B,
                    offset: 0x10,
                    type_argument_index: 1
                }
            )
        },
    },
];

/// The first row for `target_type`, so the tests that want one particular
/// target shape name it rather than depending on the table's order.
fn target(target_type: u8) -> &'static TypeTarget {
    TYPE_TARGETS
        .iter()
        .find(|t| t.target_type == target_type)
        .unwrap_or_else(|| panic!("no row for target_type 0x{target_type:02x}"))
}

/// Every target_type that is legal only inside a Code attribute.
const CODE_ONLY_TARGETS: &[u8] =
    &[0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B];

/// Every target_type that belongs on a class, field, method or record
/// component, and so may not appear inside a Code attribute.
const DECLARATION_TARGETS: &[u8] = &[0x00, 0x01, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17];

/// A type annotations body with one type_annotation: the given target, the
/// given type_path, and either no pairs or one "value" = `I p.i`.
fn type_annotation_body(p: &Anno, target_type: u8, info: &[u8], path: &[(u8, u8)], with_pair: bool) -> Bytes {
    let mut b = Bytes::new();
    b.u2(1); // num_annotations
    b.u1(target_type);
    b.raw(info);
    b.u1(path.len() as u8);
    for (kind, index) in path {
        b.u1(*kind);
        b.u1(*index);
    }
    b.u2(p.ty);
    if with_pair {
        b.u2(1);
        b.u2(p.name);
        b.u1(b'I');
        b.u2(p.i);
    } else {
        b.u2(0);
    }
    b
}

/// The type_path the C suite used: into an array, onto a wildcard bound, then
/// into type argument #1.
const SAMPLE_PATH: &[(u8, u8)] = &[(0, 0), (2, 0), (3, 1)];

#[test]
fn type_annotations_every_target_type() {
    // The target_info shape depends entirely on the target_type byte, so a
    // parser that mis-sizes one silently mis-reads everything after it.
    let p = Anno::new();
    for t in TYPE_TARGETS {
        let body = type_annotation_body(&p, t.target_type, t.info, &[], false);
        let bytes = p.attr_at(t.place, p.rvta, &body.data);
        assert_parses(&bytes, |cf| {
            let annotations = type_annotations_at(cf, t.place, true);
            assert_eq!(annotations.len(), 1);
            let ta = &annotations[0];
            assert!(
                (t.check)(&ta.target),
                "target_type 0x{:02x} ({}) parsed into the wrong target",
                t.target_type,
                t.what
            );
            assert!(ta.target_path.is_empty());
            assert_utf8_eq(ta.type_descriptor, "LAnno;");
            assert!(ta.element_value_pairs.is_empty());
        });
    }
}

#[test]
fn type_annotations_with_type_path_and_pairs() {
    let p = Anno::new();
    for t in TYPE_TARGETS {
        let body = type_annotation_body(&p, t.target_type, t.info, SAMPLE_PATH, true);
        let bytes = p.attr_at(t.place, p.rita, &body.data);
        assert_parses(&bytes, |cf| {
            let ta = &type_annotations_at(cf, t.place, false)[0];
            assert!(
                (t.check)(&ta.target),
                "target_type 0x{:02x} ({}) with a type_path parsed into the wrong target",
                t.target_type,
                t.what
            );
            let path: Vec<(u8, u8)> = ta
                .target_path
                .iter()
                .map(|e| (e.type_path_kind, e.type_argument_index))
                .collect();
            assert_eq!(path, SAMPLE_PATH);
            assert_eq!(ta.element_value_pairs.len(), 1);
            assert_utf8_eq(ta.element_value_pairs[0].element_name, "value");
            assert_eq!(const_index(&ta.element_value_pairs[0].value), p.i);
        });
    }
}

#[test]
fn type_annotations_type_path_kinds() {
    // type_argument_index is only meaningful for kind 3, and must be preserved
    // verbatim for the others rather than normalised away (JVMS 4.7.20.2).
    let p = Anno::new();
    for path in [
        vec![(0u8, 0u8)],
        vec![(1, 0)],
        vec![(2, 0)],
        vec![(3, 0)],
        vec![(3, 255)],
        vec![(0, 0), (0, 0), (1, 0), (3, 7)],
    ] {
        // 0x13 is the field target, so the attribute has to sit on a field.
        let body = type_annotation_body(&p, 0x13, &[], &path, false);
        let bytes = p.field_attr(p.rvta, &body.data);
        assert_parses(&bytes, |cf| {
            let actual: Vec<(u8, u8)> = type_annotations_at(cf, Place::Field, true)[0]
                .target_path
                .iter()
                .map(|e| (e.type_path_kind, e.type_argument_index))
                .collect();
            assert_eq!(actual, path);
        });
    }
}

#[test]
fn type_annotations_multiple_in_one_attribute() {
    // num_annotations counts type_annotations of different target shapes, so
    // each has to be sized correctly for the next one to start in the right place.
    // The three have to share a home, so they are all Code-only targets: a
    // table, a bare index, and an offset with an index after it.
    let p = Anno::new();
    let mixed = [target(0x40), target(0x42), target(0x47)];
    let mut b = Bytes::new();
    b.u2(3);
    for t in mixed {
        let one = type_annotation_body(&p, t.target_type, t.info, SAMPLE_PATH, true);
        b.raw(&one.data[2..]); // drop the per-body num_annotations
    }

    assert_parses(&p.code_attr(p.rvta, &b.data), |cf| {
        let annotations = type_annotations_at(cf, Place::Code, true);
        assert_eq!(annotations.len(), 3);
        for (ta, t) in annotations.iter().zip(mixed) {
            assert!((t.check)(&ta.target), "{} parsed into the wrong target", t.what);
        }
    });
}

#[test]
fn type_annotations_visible_and_invisible_have_distinct_tags() {
    // The two attributes share a body format but must not collapse into one
    // variant: retention is the whole point of the distinction.
    let p = Anno::new();
    let t = target(0x00);
    let body = type_annotation_body(&p, t.target_type, t.info, &[], false);

    assert_parses(&p.class_attr(p.rvta, &body.data), |cf| {
        assert!(cf
            .attributes
            .iter()
            .any(|a| matches!(a, AttributeInfo::RuntimeVisibleTypeAnnotations { .. })));
        assert!(!cf
            .attributes
            .iter()
            .any(|a| matches!(a, AttributeInfo::RuntimeInvisibleTypeAnnotations { .. })));
    });
    assert_parses(&p.class_attr(p.rita, &body.data), |cf| {
        assert!(cf
            .attributes
            .iter()
            .any(|a| matches!(a, AttributeInfo::RuntimeInvisibleTypeAnnotations { .. })));
        assert!(!cf
            .attributes
            .iter()
            .any(|a| matches!(a, AttributeInfo::RuntimeVisibleTypeAnnotations { .. })));
    });
}

#[test]
fn type_annotations_invalid_target_type_rejected() {
    // The defined bytes are not contiguous, so the gaps either side of each run
    // are what a range check is most likely to get wrong.
    let p = Anno::new();
    for bad in [0x02u8, 0x18, 0x3F, 0x4C, 0xFF] {
        let body = type_annotation_body(&p, bad, &[0, 0], &[], false);
        let bytes = p.class_attr(p.rvta, &body.data);
        assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidTypeAnnotationTarget);
    }
}

#[test]
fn code_only_target_in_class_attribute_rejected() {
    // These targets name bytecode offsets, local variable ranges and exception
    // table entries. On a class there is nothing for them to denote, so the byte
    // is well known but illegal here.
    let p = Anno::new();
    for &target_type in CODE_ONLY_TARGETS {
        let t = target(target_type);
        let body = type_annotation_body(&p, t.target_type, t.info, &[], false);
        let bytes = p.class_attr(p.rvta, &body.data);
        assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
    }
}

#[test]
fn declaration_target_in_code_attribute_rejected() {
    // The converse: a target that annotates a declaration — a type parameter, a
    // supertype, a field, a throws clause — cannot be reached from inside a
    // method body.
    let p = Anno::new();
    for &target_type in DECLARATION_TARGETS {
        let t = target(target_type);
        let body = type_annotation_body(&p, t.target_type, t.info, &[], false);
        let bytes = p.code_attr(p.rvta, &body.data);
        assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
    }
}

#[test]
fn type_annotations_truncated_rejected() {
    // The localvar target carries a nested table, the widest target_info there
    // is, so its offsets are the most interesting ones to cut at.
    let p = Anno::new();
    let t = target(0x40);
    let body = type_annotation_body(&p, t.target_type, t.info, SAMPLE_PATH, true);

    assert_every_prefix_rejected(&body, "type annotation", |cut| p.code_attr(p.rvta, cut));
}

// ---------------------------------------------------------------------------
// Through the whole class file
// ---------------------------------------------------------------------------

#[test]
fn annotations_on_class_field_method_and_parameters() {
    // The same attribute name appears at three levels; each has its own
    // attributes table and none may leak into another.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let rva = cb.pool.utf8("RuntimeVisibleAnnotations");
    let rvpa = cb.pool.utf8("RuntimeVisibleParameterAnnotations");
    let ty = cb.pool.utf8("Ljava/lang/Deprecated;");
    let name = cb.pool.utf8("since");
    let since = cb.pool.utf8("9");

    cb.reserve_attributes(1);
    let body = cb.attributes.attr_begin(rva);
    cb.attributes.u2(1);
    cb.attributes.u2(ty);
    cb.attributes.u2(1);
    cb.attributes.u2(name);
    cb.attributes.u1(b's');
    cb.attributes.u2(since);
    cb.attributes.attr_end(body);

    cb.add_field(acc::PUBLIC, "f", "I", 1);
    let body = cb.fields.attr_begin(rva);
    cb.fields.u2(1);
    write_marker_annotation(&mut cb.fields, ty);
    cb.fields.attr_end(body);

    cb.access_flags |= acc::ABSTRACT;
    cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", "(II)V", 2);
    let body = cb.methods.attr_begin(rva);
    cb.methods.u2(1);
    write_marker_annotation(&mut cb.methods, ty);
    cb.methods.attr_end(body);
    let body = cb.methods.attr_begin(rvpa);
    cb.methods.u1(2);
    cb.methods.u2(0);
    cb.methods.u2(1);
    write_marker_annotation(&mut cb.methods, ty);
    cb.methods.attr_end(body);

    assert_parses(&cb.to_bytes(), |cf| {
        let class_annotation = &visible_annotations(cf)[0];
        assert_utf8_eq(class_annotation.type_descriptor, "Ljava/lang/Deprecated;");
        let pair = &class_annotation.element_value_pairs[0];
        assert_utf8_eq(pair.element_name, "since");
        match cf.constant(const_index(&pair.value)) {
            Some(ConstantPoolEntry::UTF8(s)) => assert_eq!(*s, "9"),
            _ => panic!("the 's' element_value does not point at a CONSTANT_Utf8"),
        }

        assert!(cf.fields[0]
            .attributes
            .iter()
            .any(|a| matches!(a, AttributeInfo::RuntimeVisibleAnnotations { .. })));
        assert!(cf.methods[0]
            .attributes
            .iter()
            .any(|a| matches!(a, AttributeInfo::RuntimeVisibleAnnotations { .. })));

        let pa = parameter_annotations(cf, true);
        assert_eq!(pa.len(), 2);
        assert!(pa[0].annotations.is_empty());
        assert_eq!(pa[1].annotations.len(), 1);
    });
}

#[test]
fn annotation_type_index_must_be_utf8() {
    // type_index is a descriptor, so a CONSTANT_Class there is a real entry of
    // the wrong kind rather than something to resolve through.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let rva = cb.pool.utf8("RuntimeVisibleAnnotations");
    let this_class = cb.this_class;
    cb.reserve_attributes(1);
    let body = cb.attributes.attr_begin(rva);
    cb.attributes.u2(1);
    write_marker_annotation(&mut cb.attributes, this_class);
    cb.attributes.attr_end(body);

    assert_rejected_with!(
        &cb.to_bytes(),
        ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
    );
}

#[test]
fn element_value_const_index_type_must_match_tag() {
    // 'J' requires a CONSTANT_Long, 's' a CONSTANT_Utf8, and so on: the tag
    // fixes which constant kind the index may name (JVMS 4.7.16.1). The index
    // is in range and names a real entry, so the complaint is about its kind.
    for (tag, wrong_is_utf8) in
        [(b'J', false), (b'D', false), (b'F', false), (b's', false), (b'I', true), (b'Z', true)]
    {
        let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
        let wrong =
            if wrong_is_utf8 { cb.pool.utf8("x") } else { cb.pool.integer(1) };
        let rva = cb.pool.utf8("RuntimeVisibleAnnotations");
        let ty = cb.pool.utf8("LAnno;");
        let name = cb.pool.utf8("value");
        cb.reserve_attributes(1);
        let body = cb.attributes.attr_begin(rva);
        cb.attributes.u2(1);
        cb.attributes.u2(ty);
        cb.attributes.u2(1);
        cb.attributes.u2(name);
        cb.attributes.u1(tag);
        cb.attributes.u2(wrong);
        cb.attributes.attr_end(body);

        let bytes = cb.to_bytes();
        assert_rejected_with!(
            &bytes,
            ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry
        );
    }
}
