//! Modules: the Module, ModulePackages and ModuleMainClass attributes
//! (JVMS 4.7.25 - 4.7.27) and the module-info class shape (JVMS 4.1).
//!
//! Ported from `c_backup/tests/test_classfile_module.c`. The C suite drove
//! `attribute_info_read` directly with made-up constant pool indices; the Rust
//! model resolves every index to a `JavaUTF8`, so each of those cases is built
//! here as a whole module-info class with real Module, Package and Class
//! constants behind the indices.

use crate::common::*;

use bytecode_vm::parser::class_file::{
    AttributeInfo, ClassFile, ClassParserError, ConstantPoolEntry, ModuleAttribute,
    ModuleExportsFlags, ModuleFlags, ModuleOpensFlags, ModuleRequiresFlags,
};

// ---------------------------------------------------------------------------
// module-info builder
// ---------------------------------------------------------------------------

/// The `ModuleInfo` helper from the C suite: a class file shaped like
/// module-info (JVMS 4.1) with the constants a typical module descriptor needs
/// already in the pool.
struct ModuleInfo {
    cb: ClassBuilder,
    /// CONSTANT_Module "com.example.app"
    module_name: u16,
    /// CONSTANT_Module "java.base"
    java_base: u16,
    /// CONSTANT_Package "com/example/app"
    pkg: u16,
    /// CONSTANT_Package "com/example/app/internal"
    internal_pkg: u16,
    /// CONSTANT_Class "com/example/Service"
    service: u16,
    /// CONSTANT_Class "com/example/app/Impl"
    impl_class: u16,
    /// Utf8 "1.0"
    version: u16,
}

impl ModuleInfo {
    fn new(major_version: u16) -> Self {
        let mut cb = ClassBuilder::empty(major_version);
        cb.access_flags = acc::MODULE;
        cb.this_class = cb.pool.class("module-info");
        cb.super_class = 0;
        let module_name = cb.pool.module("com.example.app");
        let java_base = cb.pool.module("java.base");
        let pkg = cb.pool.package("com/example/app");
        let internal_pkg = cb.pool.package("com/example/app/internal");
        let service = cb.pool.class("com/example/Service");
        let impl_class = cb.pool.class("com/example/app/Impl");
        let version = cb.pool.utf8("1.0");
        ModuleInfo {
            cb,
            module_name,
            java_base,
            pkg,
            internal_pkg,
            service,
            impl_class,
            version,
        }
    }

    /// Reserves one more class attribute and starts it; pair with `attr_end`.
    fn attr_begin(&mut self, name: &str) -> usize {
        self.cb.reserve_attributes(1);
        let n = self.cb.pool.utf8(name);
        self.cb.attributes.attr_begin(n)
    }

    fn attr_end(&mut self, body: usize) {
        self.cb.attributes.attr_end(body);
    }

    fn u2(&mut self, v: u16) {
        self.cb.attributes.u2(v);
    }

    /// The Module attribute a real descriptor would carry: one requires, one
    /// exports, one qualified opens, one uses and one provides.
    fn add_module_attr(&mut self) {
        let body = self.attr_begin("Module");
        self.u2(self.module_name);
        self.u2(0);
        self.u2(self.version);

        self.u2(1); // requires_count
        self.u2(self.java_base);
        self.u2(acc::MANDATED);
        self.u2(0);

        self.u2(1); // exports_count
        self.u2(self.pkg);
        self.u2(0);
        self.u2(0);

        self.u2(1); // opens_count
        self.u2(self.internal_pkg);
        self.u2(0);
        self.u2(1);
        self.u2(self.java_base);

        self.u2(1); // uses_count
        self.u2(self.service);

        self.u2(1); // provides_count
        self.u2(self.service);
        self.u2(1);
        self.u2(self.impl_class);

        self.attr_end(body);
    }

    /// Like `add_module_attr`, but with `index` written into the slot `slot`
    /// names. Every index field of a Module attribute is reachable this way, so
    /// a bad index can be probed in one field at a time while the rest of the
    /// attribute stays well formed and the error can only come from that field.
    fn add_module_attr_with(&mut self, slot: Slot, index: u16) {
        let at = |s: Slot, default: u16| if s == slot { index } else { default };

        let body = self.attr_begin("Module");
        self.u2(at(Slot::ModuleName, self.module_name));
        self.u2(0);
        self.u2(at(Slot::ModuleVersion, 0));

        self.u2(1); // requires_count
        self.u2(at(Slot::Requires, self.java_base));
        self.u2(acc::MANDATED);
        self.u2(at(Slot::RequiresVersion, 0));

        self.u2(1); // exports_count
        self.u2(at(Slot::Exports, self.pkg));
        self.u2(0);
        self.u2(1); // exports_to_count
        self.u2(at(Slot::ExportsTo, self.java_base));

        self.u2(1); // opens_count
        self.u2(at(Slot::Opens, self.internal_pkg));
        self.u2(0);
        self.u2(1); // opens_to_count
        self.u2(at(Slot::OpensTo, self.java_base));

        self.u2(1); // uses_count
        self.u2(at(Slot::Uses, self.service));

        self.u2(1); // provides_count
        self.u2(at(Slot::Provides, self.service));
        self.u2(1); // provides_with_count
        self.u2(at(Slot::ProvidesWith, self.impl_class));

        self.attr_end(body);
    }

    /// An index one past the last constant, which names no entry at all.
    fn past_pool_end(&self) -> u16 {
        self.cb.pool.next
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.cb.to_bytes()
    }
}

/// The index fields of a Module attribute (JVMS 4.7.25).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    ModuleName,
    ModuleVersion,
    Requires,
    RequiresVersion,
    Exports,
    ExportsTo,
    Opens,
    OpensTo,
    Uses,
    Provides,
    ProvidesWith,
}

/// A Module attribute naming `module_index` with every table empty. Used on
/// plain classes too, so it takes the builder rather than a `ModuleInfo`.
fn write_empty_module_attr(cb: &mut ClassBuilder, module_index: u16) {
    cb.reserve_attributes(1);
    let name = cb.pool.utf8("Module");
    let body = cb.attributes.attr_begin(name);
    cb.attributes.u2(module_index); // module_name_index
    cb.attributes.u2(0); // module_flags
    cb.attributes.u2(0); // module_version_index
    for _ in 0..5 {
        cb.attributes.u2(0); // requires/exports/opens/uses/provides counts
    }
    cb.attributes.attr_end(body);
}

// ---------------------------------------------------------------------------
// Attribute lookup
// ---------------------------------------------------------------------------

#[track_caller]
fn module_attr<'c, 'a>(cf: &'c ClassFile<'a>) -> &'c ModuleAttribute<'a> {
    cf.attributes
        .iter()
        .find_map(|a| match a {
            AttributeInfo::Module { value } => Some(value),
            _ => None,
        })
        .expect("the class should have a Module attribute")
}

#[track_caller]
fn module_packages<'c, 'a>(cf: &'c ClassFile<'a>) -> &'c [bytecode_vm::java_utf::JavaUTF8<'a>] {
    cf.attributes
        .iter()
        .find_map(|a| match a {
            AttributeInfo::ModulePackages { value } => Some(value.as_slice()),
            _ => None,
        })
        .expect("the class should have a ModulePackages attribute")
}

#[track_caller]
fn module_main_class<'a>(cf: &ClassFile<'a>) -> bytecode_vm::java_utf::JavaUTF8<'a> {
    cf.attributes
        .iter()
        .find_map(|a| match a {
            AttributeInfo::ModuleMainClass { value } => Some(*value),
            _ => None,
        })
        .expect("the class should have a ModuleMainClass attribute")
}

/// Asserts the pool entry at `index` is a CONSTANT_Module naming `expected`.
#[track_caller]
fn assert_module_constant(cf: &ClassFile<'_>, index: u16, expected: &str) {
    match cf.constant(index) {
        Some(ConstantPoolEntry::ModuleIndex(name)) => assert_pool_utf8(cf, *name, expected),
        _ => panic!("constant {index} should be a CONSTANT_Module"),
    }
}

/// Asserts the pool entry at `index` is a CONSTANT_Package naming `expected`.
#[track_caller]
fn assert_package_constant(cf: &ClassFile<'_>, index: u16, expected: &str) {
    match cf.constant(index) {
        Some(ConstantPoolEntry::PackageIndex(name)) => assert_pool_utf8(cf, *name, expected),
        _ => panic!("constant {index} should be a CONSTANT_Package"),
    }
}

#[track_caller]
fn assert_pool_utf8(cf: &ClassFile<'_>, index: u16, expected: &str) {
    match cf.constant(index) {
        Some(ConstantPoolEntry::UTF8(s)) => assert_eq!(*s, expected),
        _ => panic!("constant {index} should be a CONSTANT_Utf8"),
    }
}

// ---------------------------------------------------------------------------
// The Module attribute (JVMS 4.7.25)
// ---------------------------------------------------------------------------

#[test]
fn module_attribute_full() {
    let mut mi = ModuleInfo::new(53);
    // Every slot gets its own constant, so a field read from the wrong offset
    // shows up as the wrong name rather than as a plausible one.
    let req_one = mi.cb.pool.module("req.one");
    let req_one_version = mi.cb.pool.utf8("9.0");
    let req_two = mi.cb.pool.module("req.two");
    let exp_a = mi.cb.pool.package("exp/a");
    let exp_b = mi.cb.pool.package("exp/b");
    let to_one = mi.cb.pool.module("to.one");
    let to_two = mi.cb.pool.module("to.two");
    let open_c = mi.cb.pool.package("open/c");
    let use_one = mi.cb.pool.class("use/One");
    let use_two = mi.cb.pool.class("use/Two");
    let svc = mi.cb.pool.class("svc/Service");
    let impl_a = mi.cb.pool.class("svc/ImplA");
    let impl_b = mi.cb.pool.class("svc/ImplB");
    let impl_c = mi.cb.pool.class("svc/ImplC");

    let body = mi.attr_begin("Module");
    mi.u2(mi.module_name);
    mi.u2(acc::OPEN);
    mi.u2(mi.version);

    mi.u2(2); // requires_count
    mi.u2(req_one);
    mi.u2(acc::MANDATED);
    mi.u2(req_one_version);
    mi.u2(req_two);
    mi.u2(acc::TRANSITIVE | acc::STATIC_PHASE);
    mi.u2(0);

    mi.u2(2); // exports_count
    mi.u2(exp_a);
    mi.u2(0);
    mi.u2(0);
    mi.u2(exp_b);
    mi.u2(acc::SYNTHETIC);
    mi.u2(2);
    mi.u2(to_one);
    mi.u2(to_two);

    mi.u2(1); // opens_count
    mi.u2(open_c);
    mi.u2(acc::MANDATED);
    mi.u2(1);
    mi.u2(to_one);

    mi.u2(2); // uses_count
    mi.u2(use_one);
    mi.u2(use_two);

    mi.u2(1); // provides_count
    mi.u2(svc);
    mi.u2(3);
    mi.u2(impl_a);
    mi.u2(impl_b);
    mi.u2(impl_c);
    mi.attr_end(body);

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        let m = module_attr(cf);
        assert_utf8_eq(m.module_name, "com.example.app");
        assert!(m.module_flags.contains(ModuleFlags::OPEN));
        assert_utf8_eq(m.module_version.expect("module_version_index was 1.0"), "1.0");

        assert_eq!(m.requires.len(), 2);
        assert_utf8_eq(m.requires[0].requires, "req.one");
        assert!(m.requires[0].requires_flags.contains(ModuleRequiresFlags::MANDATED));
        assert_utf8_eq(
            m.requires[0].requires_version.expect("requires_version_index was 9.0"),
            "9.0",
        );
        // requires_version sits between the two entries; if it isn't consumed
        // the second entry reads as {9.0's index, req.two's index} instead.
        assert_utf8_eq(m.requires[1].requires, "req.two");
        assert!(m.requires[1].requires_flags.contains(ModuleRequiresFlags::TRANSITIVE));
        assert!(m.requires[1].requires_flags.contains(ModuleRequiresFlags::STATIC_PHASE));
        // Index 0 means the requires has no version (JVMS 4.7.25).
        assert!(m.requires[1].requires_version.is_none());

        assert_eq!(m.exports.len(), 2);
        assert_utf8_eq(m.exports[0].exports, "exp/a");
        assert!(m.exports[0].exports_to.is_empty());
        assert_utf8_eq(m.exports[1].exports, "exp/b");
        assert!(m.exports[1].exports_flags.contains(ModuleExportsFlags::SYNTHETIC));
        assert_eq!(m.exports[1].exports_to.len(), 2);
        assert_utf8_eq(m.exports[1].exports_to[0], "to.one");
        assert_utf8_eq(m.exports[1].exports_to[1], "to.two");

        assert_eq!(m.opens.len(), 1);
        assert_utf8_eq(m.opens[0].opens, "open/c");
        assert!(m.opens[0].opens_flags.contains(ModuleOpensFlags::MANDATED));
        assert_eq!(m.opens[0].opens_to.len(), 1);
        assert_utf8_eq(m.opens[0].opens_to[0], "to.one");

        assert_eq!(m.uses.len(), 2);
        assert_utf8_eq(m.uses[0], "use/One");
        assert_utf8_eq(m.uses[1], "use/Two");

        assert_eq!(m.provides.len(), 1);
        assert_utf8_eq(m.provides[0].provides, "svc/Service");
        assert_eq!(m.provides[0].provides_with.len(), 3);
        assert_utf8_eq(m.provides[0].provides_with[2], "svc/ImplC");
    });
}

#[test]
fn module_attribute_empty_tables() {
    let mut mi = ModuleInfo::new(53);
    let module_name = mi.module_name;
    write_empty_module_attr(&mut mi.cb, module_name);

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        let m = module_attr(cf);
        // A module can legally require, export, open, use and provide nothing.
        assert!(m.requires.is_empty());
        assert!(m.exports.is_empty());
        assert!(m.opens.is_empty());
        assert!(m.uses.is_empty());
        assert!(m.provides.is_empty());
        // module_version_index 0 means the module carries no version.
        assert!(m.module_version.is_none());
        assert_eq!(m.module_flags, ModuleFlags::empty());
    });
}

#[test]
fn module_attribute_truncated_at_every_offset_rejected() {
    // Every table holds one entry with a non-empty sub-table, so the cut lands
    // inside a count, an index and a flags word in turn.
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();

    let bytes = mi.to_bytes();
    // The Module attribute is the only class attribute, so it runs to the end
    // of the file: cutting anywhere past its start cuts the attribute.
    let attr_start = bytes.len() - mi.cb.attributes.len();
    for len in attr_start..bytes.len() {
        let cut = &bytes[..len];
        // Each cut leaves the attribute claiming more bytes than the file has,
        // which is running off the end of the file rather than a length that
        // disagrees with contents that are all present.
        with_parsed(cut, |result| {
            assert!(
                matches!(result, Err(ClassParserError::Truncated(_))),
                "a Module attribute cut to {len} of {} bytes gave {result:?}",
                bytes.len()
            );
        });
    }
}

#[test]
fn module_attribute_length_longer_than_body_rejected() {
    let mut mi = ModuleInfo::new(53);
    let module_name = mi.module_name;
    let body = mi.attr_begin("Module");
    mi.u2(module_name);
    mi.u2(0);
    mi.u2(0);
    for _ in 0..5 {
        mi.u2(0);
    }
    // Two bytes the attribute's own structure does not account for: the
    // declared length must match the contents exactly (JVMS 4.7.1).
    mi.u2(0);
    mi.attr_end(body);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn module_attribute_length_shorter_than_body_rejected() {
    let mut mi = ModuleInfo::new(53);
    let module_name = mi.module_name;
    let body = mi.attr_begin("Module");
    mi.u2(module_name);
    mi.u2(0);
    mi.u2(0);
    for _ in 0..5 {
        mi.u2(0);
    }
    mi.attr_end(body);
    // Claim the attribute stops after module_version_index, leaving the five
    // table counts outside it.
    mi.cb.attributes.patch_u4(body - 4, 6);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn module_flag_bits() {
    // Each flag the JVMS defines for a module, its requires, its exports and
    // its opens, set on a distinct entry so no two can be confused.
    let mut mi = ModuleInfo::new(53);
    let req_transitive = mi.cb.pool.module("req.transitive");
    let req_static = mi.cb.pool.module("req.static");
    let req_synthetic = mi.cb.pool.module("req.synthetic");
    let req_mandated = mi.cb.pool.module("req.mandated");
    let exp_synthetic = mi.cb.pool.package("exp/synthetic");
    let exp_mandated = mi.cb.pool.package("exp/mandated");
    let open_synthetic = mi.cb.pool.package("open/synthetic");
    let open_mandated = mi.cb.pool.package("open/mandated");

    let body = mi.attr_begin("Module");
    mi.u2(mi.module_name);
    mi.u2(acc::OPEN);
    mi.u2(0);

    mi.u2(4); // requires_count
    for (index, flags) in [
        (req_transitive, acc::TRANSITIVE),
        (req_static, acc::STATIC_PHASE),
        (req_synthetic, acc::SYNTHETIC),
        (req_mandated, acc::MANDATED),
    ] {
        mi.u2(index);
        mi.u2(flags);
        mi.u2(0);
    }

    mi.u2(2); // exports_count
    for (index, flags) in [(exp_synthetic, acc::SYNTHETIC), (exp_mandated, acc::MANDATED)] {
        mi.u2(index);
        mi.u2(flags);
        mi.u2(0);
    }

    mi.u2(2); // opens_count
    for (index, flags) in [(open_synthetic, acc::SYNTHETIC), (open_mandated, acc::MANDATED)] {
        mi.u2(index);
        mi.u2(flags);
        mi.u2(0);
    }

    mi.u2(0); // uses_count
    mi.u2(0); // provides_count
    mi.attr_end(body);

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        let m = module_attr(cf);
        assert!(m.module_flags.contains(ModuleFlags::OPEN));
        // ACC_OPEN alone must not read as any of the other module flags.
        assert!(!m.module_flags.contains(ModuleFlags::SYNTHETIC));
        assert!(!m.module_flags.contains(ModuleFlags::MANDATED));

        assert!(m.requires[0].requires_flags.contains(ModuleRequiresFlags::TRANSITIVE));
        assert!(m.requires[1].requires_flags.contains(ModuleRequiresFlags::STATIC_PHASE));
        assert!(m.requires[2].requires_flags.contains(ModuleRequiresFlags::SYNTHETIC));
        assert!(m.requires[3].requires_flags.contains(ModuleRequiresFlags::MANDATED));
        // TRANSITIVE (0x0020) and STATIC_PHASE (0x0040) are adjacent bits.
        assert!(!m.requires[0].requires_flags.contains(ModuleRequiresFlags::STATIC_PHASE));
        assert!(!m.requires[1].requires_flags.contains(ModuleRequiresFlags::TRANSITIVE));

        assert!(m.exports[0].exports_flags.contains(ModuleExportsFlags::SYNTHETIC));
        assert!(m.exports[1].exports_flags.contains(ModuleExportsFlags::MANDATED));
        assert!(!m.exports[0].exports_flags.contains(ModuleExportsFlags::MANDATED));

        assert!(m.opens[0].opens_flags.contains(ModuleOpensFlags::SYNTHETIC));
        assert!(m.opens[1].opens_flags.contains(ModuleOpensFlags::MANDATED));
        assert!(!m.opens[0].opens_flags.contains(ModuleOpensFlags::MANDATED));
    });
}

#[test]
fn module_attribute_with_every_index_field_populated() {
    // The baseline the index tests below perturb one field of. If it were not
    // itself well formed, every one of them would pass for the wrong reason.
    let mut mi = ModuleInfo::new(53);
    let version = mi.version;
    mi.add_module_attr_with(Slot::ModuleVersion, version);

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        let m = module_attr(cf);
        assert_utf8_eq(m.module_name, "com.example.app");
        assert_utf8_eq(m.module_version.expect("module_version_index was 1.0"), "1.0");
        assert_utf8_eq(m.requires[0].requires, "java.base");
        assert_utf8_eq(m.exports[0].exports_to[0], "java.base");
        assert_utf8_eq(m.opens[0].opens_to[0], "java.base");
        assert_utf8_eq(m.uses[0], "com/example/Service");
        assert_utf8_eq(m.provides[0].provides_with[0], "com/example/app/Impl");
    });
}

// Every index field of a Module attribute is checked twice: once with an index
// that names no entry at all, and once with an in-range index naming an entry
// of the wrong kind. The two are different errors, and a parser that resolves
// an index without checking its tag only fails the second.

#[test]
fn module_name_index_zero_rejected() {
    // module_name_index is not optional (JVMS 4.7.25).
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr_with(Slot::ModuleName, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_name_index_must_be_module_constant() {
    let mut mi = ModuleInfo::new(53);
    let not_a_module = mi.cb.pool.utf8("com.example.app"); // the name, not a CONSTANT_Module
    mi.add_module_attr_with(Slot::ModuleName, not_a_module);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_version_index_past_pool_end_rejected() {
    // module_version_index is optional, so 0 is legal here and an index past the
    // end of the pool is what makes it unusable.
    let mut mi = ModuleInfo::new(53);
    let past_end = mi.past_pool_end();
    mi.add_module_attr_with(Slot::ModuleVersion, past_end);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_version_index_must_be_utf8_constant() {
    let mut mi = ModuleInfo::new(53);
    let pkg = mi.pkg; // a Package, not a Utf8
    mi.add_module_attr_with(Slot::ModuleVersion, pkg);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_requires_index_zero_rejected() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr_with(Slot::Requires, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_requires_index_must_be_module_constant() {
    let mut mi = ModuleInfo::new(53);
    let pkg = mi.pkg; // a Package, not a Module
    mi.add_module_attr_with(Slot::Requires, pkg);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_requires_version_index_past_pool_end_rejected() {
    // requires_version_index is optional like module_version_index.
    let mut mi = ModuleInfo::new(53);
    let past_end = mi.past_pool_end();
    mi.add_module_attr_with(Slot::RequiresVersion, past_end);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_requires_version_index_must_be_utf8_constant() {
    let mut mi = ModuleInfo::new(53);
    let pkg = mi.pkg; // a Package, not a Utf8
    mi.add_module_attr_with(Slot::RequiresVersion, pkg);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_exports_index_zero_rejected() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr_with(Slot::Exports, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_exports_index_must_be_package_constant() {
    let mut mi = ModuleInfo::new(53);
    let java_base = mi.java_base; // a Module, not a Package
    mi.add_module_attr_with(Slot::Exports, java_base);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_exports_to_index_zero_rejected() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr_with(Slot::ExportsTo, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_exports_to_index_must_be_module_constant() {
    let mut mi = ModuleInfo::new(53);
    let internal_pkg = mi.internal_pkg; // a Package, not a Module
    mi.add_module_attr_with(Slot::ExportsTo, internal_pkg);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_opens_index_zero_rejected() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr_with(Slot::Opens, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_opens_index_must_be_package_constant() {
    let mut mi = ModuleInfo::new(53);
    let service = mi.service; // a Class, not a Package
    mi.add_module_attr_with(Slot::Opens, service);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_opens_to_index_zero_rejected() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr_with(Slot::OpensTo, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_opens_to_index_must_be_module_constant() {
    let mut mi = ModuleInfo::new(53);
    let internal_pkg = mi.internal_pkg; // a Package, not a Module
    mi.add_module_attr_with(Slot::OpensTo, internal_pkg);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_uses_index_zero_rejected() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr_with(Slot::Uses, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_uses_index_must_be_class_constant() {
    let mut mi = ModuleInfo::new(53);
    let pkg = mi.pkg; // a Package, not a Class
    mi.add_module_attr_with(Slot::Uses, pkg);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_provides_index_zero_rejected() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr_with(Slot::Provides, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_provides_index_must_be_class_constant() {
    let mut mi = ModuleInfo::new(53);
    let java_base = mi.java_base; // a Module, not a Class
    mi.add_module_attr_with(Slot::Provides, java_base);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_provides_with_index_zero_rejected() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr_with(Slot::ProvidesWith, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn module_provides_with_index_must_be_class_constant() {
    let mut mi = ModuleInfo::new(53);
    let java_base = mi.java_base; // a Module, not a Class
    mi.add_module_attr_with(Slot::ProvidesWith, java_base);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_provides_with_must_not_be_empty() {
    // JVMS 4.7.25 requires provides_with_count > 0: a service with no
    // implementation is not a provides.
    let mut mi = ModuleInfo::new(53);
    let body = mi.attr_begin("Module");
    mi.u2(mi.module_name);
    mi.u2(0);
    mi.u2(0);
    for _ in 0..4 {
        mi.u2(0);
    }
    mi.u2(1); // provides_count
    mi.u2(mi.service);
    mi.u2(0); // provides_with_count
    mi.attr_end(body);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_attribute_outside_module_info_rejected() {
    // JVMS 4.7.25: Module may appear only in a class with ACC_MODULE set.
    // HotSpot is more lenient about a misplaced attribute, so
    // `tests/jvm/CrossCheck.java` is expected to disagree here.
    let mut cb = ClassBuilder::new(53);
    let module_name = cb.pool.module("com.example.app");
    write_empty_module_attr(&mut cb, module_name);

    let bytes = cb.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

// ---------------------------------------------------------------------------
// The ModulePackages attribute (JVMS 4.7.26)
// ---------------------------------------------------------------------------

#[test]
fn module_packages_attribute() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    let (pkg, internal_pkg) = (mi.pkg, mi.internal_pkg);
    let name = mi.cb.pool.utf8("ModulePackages");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2_table(name, &[pkg, internal_pkg]);

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        let packages = module_packages(cf);
        assert_eq!(packages.len(), 2);
        assert_utf8_eq(packages[0], "com/example/app");
        assert_utf8_eq(packages[1], "com/example/app/internal");
    });
}

#[test]
fn module_packages_entry_must_be_package_constant() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    let java_base = mi.java_base; // a Module, not a Package
    let name = mi.cb.pool.utf8("ModulePackages");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2_table(name, &[java_base]);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn module_packages_entry_zero_rejected() {
    // Every entry of the table is a package_index; none of them is optional.
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    let name = mi.cb.pool.utf8("ModulePackages");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2_table(name, &[0]);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn two_module_packages_attributes_rejected() {
    // JVMS 4.7.26 allows at most one ModulePackages attribute.
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    for _ in 0..2 {
        let pkg = mi.pkg;
        let name = mi.cb.pool.utf8("ModulePackages");
        mi.cb.reserve_attributes(1);
        mi.cb.attributes.attr_u2_table(name, &[pkg]);
    }

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_packages_outside_module_info_rejected() {
    // JVMS 4.7.26 places ModulePackages in module-info only. HotSpot is more
    // lenient about a misplaced attribute, so `tests/jvm/CrossCheck.java` is
    // expected to disagree here.
    let mut cb = ClassBuilder::new(53);
    let pkg = cb.pool.package("com/example/app");
    let name = cb.pool.utf8("ModulePackages");
    cb.reserve_attributes(1);
    cb.attributes.attr_u2_table(name, &[pkg]);

    let bytes = cb.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

// ---------------------------------------------------------------------------
// The ModuleMainClass attribute (JVMS 4.7.27)
// ---------------------------------------------------------------------------

#[test]
fn module_main_class_attribute() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    let main = mi.cb.pool.class("app/Main");
    let name = mi.cb.pool.utf8("ModuleMainClass");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2(name, main);

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        assert_utf8_eq(module_main_class(cf), "app/Main");
    });
}

#[test]
fn module_main_class_must_be_class_constant() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    let pkg = mi.pkg; // a Package, not a Class
    let name = mi.cb.pool.utf8("ModuleMainClass");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2(name, pkg);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn two_module_main_class_attributes_rejected() {
    // JVMS 4.7.27 allows at most one ModuleMainClass attribute.
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    for _ in 0..2 {
        let main = mi.impl_class;
        let name = mi.cb.pool.utf8("ModuleMainClass");
        mi.cb.reserve_attributes(1);
        mi.cb.attributes.attr_u2(name, main);
    }

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_main_class_outside_module_info_rejected() {
    // JVMS 4.7.27 places ModuleMainClass in module-info only; an attribute in a
    // location it may not appear in is a format error. HotSpot is more lenient
    // and loads this class, so `tests/jvm/CrossCheck.java` is expected to
    // disagree here.
    let mut cb = ClassBuilder::new(53);
    let main = cb.pool.class("app/Main");
    let name = cb.pool.utf8("ModuleMainClass");
    cb.reserve_attributes(1);
    cb.attributes.attr_u2(name, main);

    let bytes = cb.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_main_class_index_zero_rejected() {
    // main_class_index is not optional (JVMS 4.7.27).
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    let name = mi.cb.pool.utf8("ModuleMainClass");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2(name, 0);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

// ---------------------------------------------------------------------------
// module-info classes (JVMS 4.1)
// ---------------------------------------------------------------------------

#[test]
fn module_info_class_parses() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    let (module_name, pkg, internal_pkg, impl_class) =
        (mi.module_name, mi.pkg, mi.internal_pkg, mi.impl_class);
    let packages_name = mi.cb.pool.utf8("ModulePackages");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2_table(packages_name, &[pkg, internal_pkg]);
    let main_name = mi.cb.pool.utf8("ModuleMainClass");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2(main_name, impl_class);

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        assert!(cf.access_flags.contains(
            bytecode_vm::parser::class_file::ClassFileAccessFlags::MODULE
        ));
        // A module-info class extends nothing at all (JVMS 4.1).
        assert!(cf.super_class.is_none());
        assert_utf8_eq(cf.this_class, "module-info");

        // CONSTANT_Module and CONSTANT_Package exist only for module-info.
        assert_module_constant(cf, module_name, "com.example.app");
        assert_package_constant(cf, pkg, "com/example/app");

        let m = module_attr(cf);
        assert_utf8_eq(m.module_name, "com.example.app");
        assert_utf8_eq(m.module_version.expect("the module has a version"), "1.0");
        assert_utf8_eq(m.requires[0].requires, "java.base");
        assert!(m.requires[0].requires_flags.contains(ModuleRequiresFlags::MANDATED));
        assert_utf8_eq(m.exports[0].exports, "com/example/app");
        assert_utf8_eq(m.opens[0].opens_to[0], "java.base");
        assert_utf8_eq(m.uses[0], "com/example/Service");
        assert_utf8_eq(m.provides[0].provides_with[0], "com/example/app/Impl");

        assert_eq!(module_packages(cf).len(), 2);
        assert_utf8_eq(module_main_class(cf), "com/example/app/Impl");
    });
}

#[test]
fn module_info_with_other_flags_rejected() {
    // ACC_MODULE must be the only flag set (JVMS 4.1).
    let mut mi = ModuleInfo::new(53);
    mi.cb.access_flags = acc::MODULE | acc::PUBLIC;
    mi.add_module_attr();

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAccessFlagsCombination);
}

#[test]
fn module_info_before_version_53_rejected() {
    // Modules arrived in Java 9; ACC_MODULE is meaningless before major 53.
    let mut mi = ModuleInfo::new(52);
    mi.add_module_attr();

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseUnsupportedVersion);
}

#[test]
fn module_info_at_the_latest_version_parses() {
    let mut mi = ModuleInfo::new(MAX_MAJOR_VERSION);
    mi.add_module_attr();

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        assert_utf8_eq(cf.this_class, "module-info");
    });
}

#[test]
fn module_info_with_wrong_this_class_rejected() {
    // The index is a perfectly good CONSTANT_Class; only the name it carries is
    // wrong, which JVMS 4.1 makes a format error rather than a bad index.
    let mut mi = ModuleInfo::new(53);
    let other = mi.cb.pool.class("Test");
    mi.cb.this_class = other;
    mi.add_module_attr();

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_info_with_super_class_rejected() {
    // java/lang/Object is a usable super_class everywhere else, so what is
    // rejected here is module-info having a super class at all (JVMS 4.1).
    let mut mi = ModuleInfo::new(53);
    let object = mi.cb.pool.class("java/lang/Object");
    mi.cb.super_class = object;
    mi.add_module_attr();

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_info_with_interfaces_fields_or_methods_rejected() {
    for which in 0..3 {
        let mut mi = ModuleInfo::new(53);
        match which {
            0 => {
                let runnable = mi.cb.pool.class("java/lang/Runnable");
                mi.cb.add_interface(runnable);
            }
            1 => mi.cb.add_field(acc::PUBLIC | acc::STATIC | acc::FINAL, "x", "I", 0),
            _ => mi.cb.add_method(acc::PUBLIC | acc::ABSTRACT, "m", "()V", 0),
        }
        mi.add_module_attr();

        let bytes = mi.to_bytes();
        let what = match which {
            0 => "an interface",
            1 => "a field",
            _ => "a method",
        };
        let err = assert_rejected(&bytes);
        assert!(
            matches!(err, ClassParserError::ClassParseFormatError),
            "module-info with {what}: expected a format error, got {err:?}"
        );
    }
}

#[test]
fn module_info_without_module_attribute_rejected() {
    // The Module attribute is what makes a module-info a module (JVMS 4.7.25).
    let mi = ModuleInfo::new(53);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_info_with_two_module_attributes_rejected() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    mi.add_module_attr();

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_info_with_disallowed_attribute_rejected() {
    // Only Module, ModulePackages, ModuleMainClass, InnerClasses, SourceFile,
    // SourceDebugExtension, RuntimeVisibleAnnotations and
    // RuntimeInvisibleAnnotations may appear in module-info.
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();
    let version = mi.version;
    let name = mi.cb.pool.utf8("Signature");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2(name, version);

    let bytes = mi.to_bytes();
    assert_rejected_with!(&bytes, ClassParserError::ClassParseFormatError);
}

#[test]
fn module_info_allowed_extra_attributes_accepted() {
    let mut mi = ModuleInfo::new(53);
    mi.add_module_attr();

    let source = mi.cb.pool.utf8("module-info.java");
    let source_file = mi.cb.pool.utf8("SourceFile");
    mi.cb.reserve_attributes(1);
    mi.cb.attributes.attr_u2(source_file, source);

    let deprecated = mi.cb.pool.utf8("Ljava/lang/Deprecated;");
    let body = mi.attr_begin("RuntimeVisibleAnnotations");
    mi.u2(1); // num_annotations
    mi.u2(deprecated);
    mi.u2(0); // num_element_value_pairs
    mi.attr_end(body);

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        assert_eq!(cf.attributes.len(), 3);
    });
}

#[test]
fn module_constants_in_module_info_accepted_without_attribute_references() {
    // Module/Package entries are legal in module-info even when unused.
    let mut mi = ModuleInfo::new(53);
    mi.cb.pool.module("unused.module");
    mi.cb.pool.package("unused/pkg");
    mi.add_module_attr();

    let bytes = mi.to_bytes();
    assert_parses(&bytes, |cf| {
        assert_utf8_eq(module_attr(cf).module_name, "com.example.app");
    });
}
