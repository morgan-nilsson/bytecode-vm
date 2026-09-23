//! Shared helpers for the class file tests: JVMS constants, a byte writer, and
//! builders for constant pools and whole class files.
//!
//! Ported from the C suite in `c_backup/tests/classfile_support.h`.
//!
//! Tests drive the parser through its public entry point:
//!
//! ```ignore
//! let cb = ClassBuilder::new(52);
//! let bytes = cb.to_bytes();
//! let mut reader = Reader::new(&bytes);
//! let cf = ClassFile::parse(&mut reader).expect("should parse");
//! ```
//!
//! The two-step is needed because `ClassFile` borrows from the byte buffer, so
//! the buffer has to outlive it.

#![allow(dead_code)]

use std::path::PathBuf;

use bytecode_vm::java_utf::JavaUTF8;
use bytecode_vm::parser::class_file::{ClassFile, ClassParserError};
use bytecode_vm::parser::reader::Reader;

// ---------------------------------------------------------------------------
// JVMS constants
// ---------------------------------------------------------------------------

pub mod tag {
    pub const UTF8: u8 = 1;
    pub const INTEGER: u8 = 3;
    pub const FLOAT: u8 = 4;
    pub const LONG: u8 = 5;
    pub const DOUBLE: u8 = 6;
    pub const CLASS: u8 = 7;
    pub const STRING: u8 = 8;
    pub const FIELDREF: u8 = 9;
    pub const METHODREF: u8 = 10;
    pub const INTERFACE_METHODREF: u8 = 11;
    pub const NAME_AND_TYPE: u8 = 12;
    pub const METHOD_HANDLE: u8 = 15;
    pub const METHOD_TYPE: u8 = 16;
    pub const DYNAMIC: u8 = 17;
    pub const INVOKE_DYNAMIC: u8 = 18;
    pub const MODULE: u8 = 19;
    pub const PACKAGE: u8 = 20;
}

pub mod refkind {
    pub const GET_FIELD: u8 = 1;
    pub const GET_STATIC: u8 = 2;
    pub const PUT_FIELD: u8 = 3;
    pub const PUT_STATIC: u8 = 4;
    pub const INVOKE_VIRTUAL: u8 = 5;
    pub const INVOKE_STATIC: u8 = 6;
    pub const INVOKE_SPECIAL: u8 = 7;
    pub const NEW_INVOKE_SPECIAL: u8 = 8;
    pub const INVOKE_INTERFACE: u8 = 9;
}

pub mod item {
    pub const TOP: u8 = 0;
    pub const INTEGER: u8 = 1;
    pub const FLOAT: u8 = 2;
    pub const DOUBLE: u8 = 3;
    pub const LONG: u8 = 4;
    pub const NULL: u8 = 5;
    pub const UNINITIALIZED_THIS: u8 = 6;
    pub const OBJECT: u8 = 7;
    pub const UNINITIALIZED: u8 = 8;
}

pub mod acc {
    pub const PUBLIC: u16 = 0x0001;
    pub const PRIVATE: u16 = 0x0002;
    pub const PROTECTED: u16 = 0x0004;
    pub const STATIC: u16 = 0x0008;
    pub const FINAL: u16 = 0x0010;
    pub const SUPER: u16 = 0x0020;
    pub const SYNCHRONIZED: u16 = 0x0020;
    pub const OPEN: u16 = 0x0020;
    pub const TRANSITIVE: u16 = 0x0020;
    pub const VOLATILE: u16 = 0x0040;
    pub const BRIDGE: u16 = 0x0040;
    pub const STATIC_PHASE: u16 = 0x0040;
    pub const TRANSIENT: u16 = 0x0080;
    pub const VARARGS: u16 = 0x0080;
    pub const NATIVE: u16 = 0x0100;
    pub const INTERFACE: u16 = 0x0200;
    pub const ABSTRACT: u16 = 0x0400;
    pub const STRICT: u16 = 0x0800;
    pub const SYNTHETIC: u16 = 0x1000;
    pub const ANNOTATION: u16 = 0x2000;
    pub const ENUM: u16 = 0x4000;
    pub const MODULE: u16 = 0x8000;
    pub const MANDATED: u16 = 0x8000;
}

/// Bytecodes used in generated Code attributes.
pub mod op {
    pub const NOP: u8 = 0x00;
    pub const ICONST_0: u8 = 0x03;
    pub const ALOAD_0: u8 = 0x2a;
    pub const IRETURN: u8 = 0xac;
    pub const RETURN: u8 = 0xb1;
    pub const INVOKESPECIAL: u8 = 0xb7;
}

/// Highest class file major version the parser accepts, inclusive: 45 (Java 1.1)
/// through 69 (Java 25). Raise this when the parser supports a newer release.
pub const MAX_MAJOR_VERSION: u16 = 69;

/// The version most tests build against unless they are testing versioning.
pub const DEFAULT_MAJOR_VERSION: u16 = 52;

// ---------------------------------------------------------------------------
// Byte writer
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Bytes {
    pub data: Vec<u8>,
}

impl Bytes {
    pub fn new() -> Self {
        Bytes { data: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn u1(&mut self, v: u8) -> &mut Self {
        self.data.push(v);
        self
    }

    pub fn u2(&mut self, v: u16) -> &mut Self {
        self.data.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn u4(&mut self, v: u32) -> &mut Self {
        self.data.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn u8_be(&mut self, v: u64) -> &mut Self {
        self.data.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.data.extend_from_slice(bytes);
        self
    }

    pub fn cat(&mut self, other: &Bytes) -> &mut Self {
        self.data.extend_from_slice(&other.data);
        self
    }

    pub fn fill(&mut self, v: u8, count: usize) -> &mut Self {
        self.data.resize(self.data.len() + count, v);
        self
    }

    pub fn patch_u2(&mut self, at: usize, v: u16) {
        self.data[at..at + 2].copy_from_slice(&v.to_be_bytes());
    }

    pub fn patch_u4(&mut self, at: usize, v: u32) {
        self.data[at..at + 4].copy_from_slice(&v.to_be_bytes());
    }

    /// A copy of the first `len` bytes, for truncation tests.
    pub fn prefix(&self, len: usize) -> Vec<u8> {
        self.data[..len.min(self.data.len())].to_vec()
    }

    // --- attribute framing -------------------------------------------------

    /// Writes `attribute_name_index` and a placeholder `attribute_length`,
    /// returning the body offset to hand to `attr_end`.
    pub fn attr_begin(&mut self, name_index: u16) -> usize {
        self.u2(name_index);
        self.u4(0);
        self.data.len()
    }

    pub fn attr_end(&mut self, body: usize) {
        let len = (self.data.len() - body) as u32;
        self.patch_u4(body - 4, len);
    }

    /// A whole attribute whose body is one u2.
    pub fn attr_u2(&mut self, name_index: u16, value: u16) -> &mut Self {
        let body = self.attr_begin(name_index);
        self.u2(value);
        self.attr_end(body);
        self
    }

    /// A whole attribute with an empty body.
    pub fn attr_empty(&mut self, name_index: u16) -> &mut Self {
        let body = self.attr_begin(name_index);
        self.attr_end(body);
        self
    }

    /// A whole attribute holding a u2 count followed by that many u2 values.
    pub fn attr_u2_table(&mut self, name_index: u16, values: &[u16]) -> &mut Self {
        let body = self.attr_begin(name_index);
        self.u2(values.len() as u16);
        for v in values {
            self.u2(*v);
        }
        self.attr_end(body);
        self
    }

    /// A whole attribute with a verbatim body.
    pub fn attr_raw(&mut self, name_index: u16, body_bytes: &[u8]) -> &mut Self {
        self.u2(name_index);
        self.u4(body_bytes.len() as u32);
        self.raw(body_bytes);
        self
    }
}

// ---------------------------------------------------------------------------
// Constant pool builder
// ---------------------------------------------------------------------------

/// Builds the serialised form of a constant pool. Every method returns the
/// index the new entry got, so tests can refer back to it.
#[derive(Default, Clone)]
pub struct PoolBuilder {
    pub bytes: Bytes,
    /// The index the next entry will get; starts at 1.
    pub next: u16,
}

impl PoolBuilder {
    pub fn new() -> Self {
        PoolBuilder { bytes: Bytes::new(), next: 1 }
    }

    pub fn utf8_bytes(&mut self, data: &[u8]) -> u16 {
        self.bytes.u1(tag::UTF8);
        self.bytes.u2(data.len() as u16);
        self.bytes.raw(data);
        let index = self.next;
        self.next += 1;
        index
    }

    pub fn utf8(&mut self, s: &str) -> u16 {
        self.utf8_bytes(s.as_bytes())
    }

    pub fn integer(&mut self, v: i32) -> u16 {
        self.bytes.u1(tag::INTEGER);
        self.bytes.u4(v as u32);
        let index = self.next;
        self.next += 1;
        index
    }

    pub fn float_bits(&mut self, bits: u32) -> u16 {
        self.bytes.u1(tag::FLOAT);
        self.bytes.u4(bits);
        let index = self.next;
        self.next += 1;
        index
    }

    /// Long and Double take two slots; the second is unusable (JVMS 4.4.5).
    pub fn long(&mut self, v: i64) -> u16 {
        self.bytes.u1(tag::LONG);
        self.bytes.u8_be(v as u64);
        let index = self.next;
        self.next += 2;
        index
    }

    pub fn double_bits(&mut self, bits: u64) -> u16 {
        self.bytes.u1(tag::DOUBLE);
        self.bytes.u8_be(bits);
        let index = self.next;
        self.next += 2;
        index
    }

    pub fn u2_entry(&mut self, entry_tag: u8, a: u16) -> u16 {
        self.bytes.u1(entry_tag);
        self.bytes.u2(a);
        let index = self.next;
        self.next += 1;
        index
    }

    pub fn u2u2_entry(&mut self, entry_tag: u8, a: u16, b: u16) -> u16 {
        self.bytes.u1(entry_tag);
        self.bytes.u2(a);
        self.bytes.u2(b);
        let index = self.next;
        self.next += 1;
        index
    }

    pub fn class_at(&mut self, name_index: u16) -> u16 {
        self.u2_entry(tag::CLASS, name_index)
    }

    pub fn class(&mut self, name: &str) -> u16 {
        let n = self.utf8(name);
        self.class_at(n)
    }

    pub fn string(&mut self, s: &str) -> u16 {
        let n = self.utf8(s);
        self.u2_entry(tag::STRING, n)
    }

    pub fn name_and_type(&mut self, name: &str, desc: &str) -> u16 {
        let n = self.utf8(name);
        let d = self.utf8(desc);
        self.u2u2_entry(tag::NAME_AND_TYPE, n, d)
    }

    pub fn member_ref(&mut self, entry_tag: u8, cls: &str, name: &str, desc: &str) -> u16 {
        let c = self.class(cls);
        let nat = self.name_and_type(name, desc);
        self.u2u2_entry(entry_tag, c, nat)
    }

    pub fn fieldref(&mut self, cls: &str, name: &str, desc: &str) -> u16 {
        self.member_ref(tag::FIELDREF, cls, name, desc)
    }

    pub fn methodref(&mut self, cls: &str, name: &str, desc: &str) -> u16 {
        self.member_ref(tag::METHODREF, cls, name, desc)
    }

    pub fn interface_methodref(&mut self, cls: &str, name: &str, desc: &str) -> u16 {
        self.member_ref(tag::INTERFACE_METHODREF, cls, name, desc)
    }

    pub fn method_handle(&mut self, kind: u8, ref_index: u16) -> u16 {
        self.bytes.u1(tag::METHOD_HANDLE);
        self.bytes.u1(kind);
        self.bytes.u2(ref_index);
        let index = self.next;
        self.next += 1;
        index
    }

    pub fn method_type(&mut self, desc: &str) -> u16 {
        let d = self.utf8(desc);
        self.u2_entry(tag::METHOD_TYPE, d)
    }

    pub fn module(&mut self, name: &str) -> u16 {
        let n = self.utf8(name);
        self.u2_entry(tag::MODULE, n)
    }

    pub fn package(&mut self, name: &str) -> u16 {
        let n = self.utf8(name);
        self.u2_entry(tag::PACKAGE, n)
    }

    /// A bootstrap method handle usable in a BootstrapMethods entry:
    /// `invokestatic Test.bsm(Lookup, String, MethodType)CallSite`.
    pub fn bootstrap_handle(&mut self) -> u16 {
        let r = self.methodref(
            "Test",
            "bsm",
            "(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;\
             Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/CallSite;",
        );
        self.method_handle(refkind::INVOKE_STATIC, r)
    }
}

// ---------------------------------------------------------------------------
// Class builder
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ClassBuilder {
    pub magic: u32,
    pub minor_version: u16,
    pub major_version: u16,
    pub pool: PoolBuilder,
    /// Set to override the count written to the file, for malformed-input tests.
    pub pool_count_override: Option<u16>,
    pub access_flags: u16,
    pub this_class: u16,
    pub super_class: u16,
    pub interfaces_count: u16,
    pub interfaces: Bytes,
    pub fields_count: u16,
    pub fields: Bytes,
    pub methods_count: u16,
    pub methods: Bytes,
    pub attributes_count: u16,
    pub attributes: Bytes,
    pub trailing: Bytes,
}

impl ClassBuilder {
    /// An empty builder: no constant pool entries, `this_class`/`super_class` 0.
    pub fn empty(major_version: u16) -> Self {
        ClassBuilder {
            magic: 0xCAFE_BABE,
            minor_version: 0,
            major_version,
            pool: PoolBuilder::new(),
            pool_count_override: None,
            access_flags: 0,
            this_class: 0,
            super_class: 0,
            interfaces_count: 0,
            interfaces: Bytes::new(),
            fields_count: 0,
            fields: Bytes::new(),
            methods_count: 0,
            methods: Bytes::new(),
            attributes_count: 0,
            attributes: Bytes::new(),
            trailing: Bytes::new(),
        }
    }

    /// `public class Test extends java.lang.Object {}` at the given version.
    pub fn new(major_version: u16) -> Self {
        let mut cb = ClassBuilder::empty(major_version);
        cb.access_flags = acc::PUBLIC | acc::SUPER;
        cb.this_class = cb.pool.class("Test");
        cb.super_class = cb.pool.class("java/lang/Object");
        cb
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Bytes::new();
        b.u4(self.magic);
        b.u2(self.minor_version);
        b.u2(self.major_version);
        b.u2(self.pool_count_override.unwrap_or(self.pool.next));
        b.cat(&self.pool.bytes);
        b.u2(self.access_flags);
        b.u2(self.this_class);
        b.u2(self.super_class);
        b.u2(self.interfaces_count);
        b.cat(&self.interfaces);
        b.u2(self.fields_count);
        b.cat(&self.fields);
        b.u2(self.methods_count);
        b.cat(&self.methods);
        b.u2(self.attributes_count);
        b.cat(&self.attributes);
        b.cat(&self.trailing);
        b.data
    }

    pub fn add_interface(&mut self, class_index: u16) -> &mut Self {
        self.interfaces.u2(class_index);
        self.interfaces_count += 1;
        self
    }

    /// Starts a `field_info`. Write exactly `attributes_count` attributes to
    /// `self.fields` before adding the next field.
    pub fn add_field(&mut self, flags: u16, name: &str, desc: &str, attributes_count: u16) {
        let n = self.pool.utf8(name);
        let d = self.pool.utf8(desc);
        self.fields.u2(flags);
        self.fields.u2(n);
        self.fields.u2(d);
        self.fields.u2(attributes_count);
        self.fields_count += 1;
    }

    /// Starts a `method_info`. Write exactly `attributes_count` attributes to
    /// `self.methods` before adding the next method.
    pub fn add_method(&mut self, flags: u16, name: &str, desc: &str, attributes_count: u16) {
        let n = self.pool.utf8(name);
        let d = self.pool.utf8(desc);
        self.methods.u2(flags);
        self.methods.u2(n);
        self.methods.u2(d);
        self.methods.u2(attributes_count);
        self.methods_count += 1;
    }

    /// A concrete method whose Code is a single return suitable for its descriptor.
    pub fn add_simple_method(&mut self, flags: u16, name: &str, desc: &str) {
        self.add_method(flags, name, desc, 1);
        let locals = descriptor_arg_slots(desc) + if flags & acc::STATIC != 0 { 0 } else { 1 };
        let code_name = self.pool.utf8("Code");
        let returns_void = desc.split(')').nth(1) == Some("V");
        let (max_stack, code): (u16, Vec<u8>) = if returns_void {
            (0, vec![op::RETURN])
        } else {
            (1, vec![op::ICONST_0, op::IRETURN])
        };
        let body = self.methods.attr_begin(code_name);
        self.methods.u2(max_stack);
        self.methods.u2(locals);
        self.methods.u4(code.len() as u32);
        self.methods.raw(&code);
        self.methods.u2(0); // exception_table_length
        self.methods.u2(0); // attributes_count
        self.methods.attr_end(body);
    }

    /// Reserves room for `count` more class-level attributes; write them to
    /// `self.attributes`.
    pub fn reserve_attributes(&mut self, count: u16) {
        self.attributes_count += count;
    }

    /// A Code attribute with the given body, no handlers and no sub-attributes.
    pub fn write_code_attr(
        out: &mut Bytes,
        pool: &mut PoolBuilder,
        max_stack: u16,
        max_locals: u16,
        code: &[u8],
    ) {
        let name = pool.utf8("Code");
        let body = out.attr_begin(name);
        out.u2(max_stack);
        out.u2(max_locals);
        out.u4(code.len() as u32);
        out.raw(code);
        out.u2(0);
        out.u2(0);
        out.attr_end(body);
    }

    /// A BootstrapMethods attribute with `count` entries, each using `handle`
    /// and taking no arguments.
    pub fn write_bootstrap_methods(
        out: &mut Bytes,
        pool: &mut PoolBuilder,
        handle: u16,
        count: u16,
    ) {
        let name = pool.utf8("BootstrapMethods");
        let body = out.attr_begin(name);
        out.u2(count);
        for _ in 0..count {
            out.u2(handle);
            out.u2(0);
        }
        out.attr_end(body);
    }
}

/// Local variable slots a method descriptor's parameters need.
pub fn descriptor_arg_slots(desc: &str) -> u16 {
    let bytes = desc.as_bytes();
    if bytes.first() != Some(&b'(') {
        return 0;
    }
    let mut slots = 0u16;
    let mut i = 1;
    while i < bytes.len() && bytes[i] != b')' {
        match bytes[i] {
            b'J' | b'D' => {
                slots += 2;
                i += 1;
            }
            _ => {
                while i < bytes.len() && bytes[i] == b'[' {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'L' {
                    while i < bytes.len() && bytes[i] != b';' {
                        i += 1;
                    }
                }
                i += 1;
                slots += 1;
            }
        }
    }
    slots
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parses `bytes`, giving the caller the borrowed `ClassFile`.
///
/// `ClassFile` borrows from the buffer, so the buffer has to outlive it; the
/// closure form keeps that tidy at the call site.
pub fn with_parsed<T>(
    bytes: &[u8],
    f: impl FnOnce(Result<ClassFile<'_>, ClassParserError>) -> T,
) -> T {
    let mut reader = Reader::new(bytes);
    f(ClassFile::parse(&mut reader))
}

/// Asserts the bytes parse, and hands the result to `f`.
#[track_caller]
pub fn assert_parses<T>(bytes: &[u8], f: impl FnOnce(&ClassFile<'_>) -> T) -> T {
    dump_for_crosscheck(bytes, "accept");
    with_parsed(bytes, |result| match result {
        Ok(cf) => f(&cf),
        Err(e) => panic!("expected the class to parse, got {e}"),
    })
}

/// Asserts the bytes are rejected, and returns the error for further checks.
#[track_caller]
pub fn assert_rejected(bytes: &[u8]) -> ClassParserError {
    dump_for_crosscheck(bytes, "reject");
    with_parsed(bytes, |result| match result {
        Ok(_) => panic!("expected the parser to reject this input, but it was accepted"),
        Err(e) => e,
    })
}

/// Cross-checking against a real JVM: with `CLASSFILE_TEST_DUMP=<dir>` set,
/// every class a test builds is written to that directory along with the
/// outcome the test expects, in `<dir>/manifest.txt`. `tests/jvm/CrossCheck.java`
/// then feeds them to `ClassLoader.defineClass` and reports disagreements,
/// which is how the test expectations themselves get validated.
#[track_caller]
fn dump_for_crosscheck(bytes: &[u8], expect: &str) {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let Ok(dir) = std::env::var("CLASSFILE_TEST_DUMP") else { return };
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = PathBuf::from(dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    // The caller's location names the test the class came from.
    let location = std::panic::Location::caller();
    let stem = location.file().replace(['/', '.'], "_");
    let path = dir.join(format!("{stem}_{}_{n}.class", location.line()));
    if std::fs::write(&path, bytes).is_err() {
        return;
    }
    // Tests run in parallel, so the line is formatted first and written with a
    // single append — `writeln!` would issue several writes and interleave.
    if let Ok(mut f) =
        std::fs::OpenOptions::new().create(true).append(true).open(dir.join("manifest.txt"))
    {
        let line = format!("{expect} {}\n", path.display());
        let _ = f.write_all(line.as_bytes());
    }
}

/// Asserts the bytes are rejected with a specific error.
#[macro_export]
macro_rules! assert_rejected_with {
    ($bytes:expr, $pattern:pat) => {{
        let err = $crate::common::assert_rejected($bytes);
        assert!(
            matches!(err, $pattern),
            "expected {}, got {err:?}",
            stringify!($pattern)
        );
        err
    }};
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Path to a class file under `tests/fixtures/classes`.
pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/classes").join(name)
}

/// Reads a fixture class file, panicking with a useful message if it is missing.
pub fn fixture(name: &str) -> Vec<u8> {
    let path = fixture_path(name);
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!("can't read fixture {}: {e} (rebuild with tests/fixtures/build.sh)", path.display())
    })
}

/// Every fixture class, as produced by javac 17.
pub const ALL_FIXTURES: &[&str] = &[
    "fixtures/Anno.class",
    "fixtures/Annotated.class",
    "fixtures/Annotated$Invisible.class",
    "fixtures/Annotated$InvisibleTypeUse.class",
    "fixtures/Annotated$TypeUse.class",
    "fixtures/Color.class",
    "fixtures/Constants.class",
    "fixtures/Hello.class",
    "fixtures/Hello$1.class",
    "fixtures/Hello$Inner.class",
    "fixtures/Hello$Nested.class",
    "fixtures/internal/Task.class",
    "fixtures/Point.class",
    "fixtures/Shape.class",
    "fixtures/Shape$Circle.class",
    "fixtures/Shape$Square.class",
    "Legacy_release7.class",
    "Legacy_release8.class",
    "module-info.class",
];

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

/// Compares a `JavaUTF8` with an expected byte string.
#[track_caller]
pub fn assert_utf8_eq(actual: JavaUTF8<'_>, expected: &str) {
    assert_eq!(
        actual.as_bytes(),
        expected.as_bytes(),
        "expected {expected:?}, got {actual:?}"
    );
}

/// Compares a `JavaUTF8` with raw bytes, for strings that are not valid UTF-8.
#[track_caller]
pub fn assert_utf8_bytes_eq(actual: JavaUTF8<'_>, expected: &[u8]) {
    assert_eq!(actual.as_bytes(), expected, "modified UTF-8 bytes differ");
}

pub fn float_bits_eq(f: f32, bits: u32) -> bool {
    f.to_bits() == bits
}

pub fn double_bits_eq(d: f64, bits: u64) -> bool {
    d.to_bits() == bits
}
