use bitflags::{bitflags};
use thiserror::Error;

use crate::java_utf::{JavaUTF8, JavaUTF8Error};
use crate::parser::reader::{ParseError, Reader};

#[derive(Error, Debug)]
pub enum ClassParserError {
    #[error("Class file contained invalid magic")]
    ClassParseInvalidMagic,

    #[error("Class file contains unsupported version")]
    ClassParseUnsupportedVersion,

    /// `constant_pool_count` itself is impossible, e.g. 0.
    #[error("Class file contains invalid constant pool")]
    ClassParseInvalidConstantPool,

    #[error("Class file contains invalid constant pool tag")]
    ClassParseInvalidConstantPoolTag,

    #[error("Class file contains invalid type annotation target type")]
    ClassParseInvalidTypeAnnotationTargetType,

    /// An index that cannot name an entry: 0, past the end of the pool, or the
    /// unusable slot after a Long or Double. Compare
    /// [`Self::ClassParseReferenceToInvalidConstantPoolEntry`], which is for an
    /// index that names a real entry of the wrong kind.
    #[error("Class file contains invalid constant pool index")]
    ClassParseInvalidConstantPoolIndex,

    /// An in-range index naming an entry of the wrong kind, e.g. a Fieldref
    /// whose `class_index` points at a Utf8 rather than a Class.
    #[error("Class file contains reference to invalid constant pool entry")]
    ClassParseReferenceToInvalidConstantPoolEntry,

    /// `this_class` is not a usable index naming a CONSTANT_Class.
    #[error("Class file contains invalid this class index")]
    ClassParseInvalidThisClassIndex,

    /// `super_class` is neither 0 nor a usable index naming a CONSTANT_Class.
    #[error("Class file contains invalid super class index")]
    ClassParseInvalidSuperClassIndex,

    #[error("Class file contains an unknown element value tag")]
    ClassParseInvalidElementValueTag,

    #[error("Class file contains an unknown verification type tag")]
    ClassParseInvalidVerificationTypeTag,

    #[error("Class file contains an unknown stack map frame type")]
    ClassParseInvalidStackMapFrameType,

    #[error("Class file contains an unknown type annotation target")]
    ClassParseInvalidTypeAnnotationTarget,

    #[error("Class file contains an invalid access flags combination")]
    ClassParseInvalidAccessFlagsCombination,

    #[error("Class file contains an attribute that is not valid for this version")]
    ClassParseInvalidFeatureUsedForVersion,

    #[error("Class file contains invalid code attribute")]
    ClassParseInvalidCodeAttribute,

    /// An attribute's contents do not fill its `attribute_length` exactly —
    /// in either direction. Running off the end of the *file* is
    /// [`Self::Truncated`] instead.
    #[error("Attribute length disagrees with the attribute's contents")]
    ClassParseInvalidAttributeLength,

    /// Bytes left over after the last attribute of the ClassFile (JVMS 4.8).
    #[error("Class file contains trailing bytes")]
    ClassParseTrailingBytes,

    /// A format check from JVMS 4.8 that has no more specific variant: an
    /// illegal access flag combination, a malformed name or descriptor, an
    /// attribute appearing where it may not or more often than it may, a
    /// duplicate field or method, and so on.
    #[error("Class file contains format error")]
    ClassParseFormatError,

    #[error("Class file ended early: {0}")]
    Truncated(#[from] ParseError),

    /// A CONSTANT_Utf8 whose bytes are not well-formed modified UTF-8.
    #[error("{0}")]
    InvalidUTF8(#[from] JavaUTF8Error),
}

fn is_array_descriptor(name: JavaUTF8) -> bool {
    name.as_bytes().first() == Some(&b'[')
}

fn valid_class_name(name: JavaUTF8) -> bool {
    !name.is_empty()
        && !name.as_bytes().starts_with(b"/") && !name.as_bytes().ends_with(b"/")
        && name.as_bytes().split(|&b| b == b'/').all(|seg| valid_segment(JavaUTF8(seg)))
}

/// Consumes one field type from the front of `d`, returning what follows, or
/// `None` if it is malformed (JVMS 4.3.2).
fn field_descriptor_tail(d: &[u8], depth: usize) -> Option<&[u8]> {
    match d.first()? {
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' => Some(&d[1..]),
        b'L' => {
            let end = d.iter().position(|&b| b == b';')?;
            if !valid_class_name(JavaUTF8(&d[1..end])) {
                return None;
            }
            Some(&d[end + 1..])
        }
        b'[' => {
            // "No more than 255 dimensions" (JVMS 4.3.2).
            if depth >= 255 {
                return None;
            }
            field_descriptor_tail(&d[1..], depth + 1)
        }
        _ => None,
    }
}

fn valid_field_descriptor(descriptor: JavaUTF8) -> bool {
    field_descriptor_tail(descriptor.as_bytes(), 0) == Some(&[][..])
}

/// The name a CONSTANT_Class may hold (JVMS 4.4.1): a binary name in internal
/// form, or — for an array type only — the array's descriptor. So a leading
/// '[' is the one case where descriptor syntax is allowed; "Ljava/lang/String;"
/// is a descriptor for a non-array type and is not a valid class name.
fn valid_class_entry_name(name: JavaUTF8) -> bool {
    if name.as_bytes().first() == Some(&b'[') {
        valid_field_descriptor(name)
    } else {
        valid_class_name(name)
    }
}

fn valid_segment(seg: JavaUTF8) -> bool {
    !seg.is_empty()                              // rejects "a//b" too
        && !seg.as_bytes().iter().any(|&b| matches!(b, b'.' | b';' | b'[' | b'/'))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassFile<'a> {
    pub minor_version: u16,
    pub major_version: u16,

    pub constant_pool: ConstantPool<'a>,

    pub access_flags: ClassFileAccessFlags,

    pub this_class: JavaUTF8<'a>,
    pub super_class: Option<JavaUTF8<'a>>,

    pub interfaces: Interfaces<'a>,

    pub fields: Fields<'a>,

    pub methods: Methods<'a>,

    pub attributes: Attributes<'a>,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ClassFileAccessFlags: u16 {
        const PUBLIC = 0x0001;
        const FINAL = 0x0010;
        const SUPER = 0x0020;
        const INTERFACE = 0x0200;
        const ABSTRACT = 0x0400;
        const SYNTHETIC = 0x1000;
        const ANNOTATION = 0x2000;
        const ENUM = 0x4000;
        const MODULE = 0x8000;
    }
}

/// What the sub-parsers need to know about the class file around them: the
/// version that gates which features are legal, and the pool and header fields
/// that indices resolve against. Borrows from the same buffer the class does.
pub struct ClassParseCtx<'a> {
    pub major_version: u16,
    pub minor_version: u16,
    pub pool: ConstantPool<'a>,
    pub access_flags: ClassFileAccessFlags,
    pub this_class: JavaUTF8<'a>,
    pub super_class: Option<JavaUTF8<'a>>,
}

impl<'a> ClassParseCtx<'a> {
    fn new(
        major_version: u16,
        minor_version: u16,
        pool: ConstantPool<'a>,
        access_flags: ClassFileAccessFlags,
        this_class: JavaUTF8<'a>,
        super_class: Option<JavaUTF8<'a>>,
    ) -> Self {
        Self {
            major_version,
            minor_version,
            pool,
            access_flags,
            this_class,
            super_class,
        }
    }

    fn at_least(&self, major: u16) -> bool {
        self.major_version >= major
    }
}

impl<'a> ClassFile<'a> {
    pub fn parse(reader: &mut Reader<'a>) -> Result<Self, ClassParserError> {

        let magic = reader.u32();
        if magic.is_err() {
            return Err(ClassParserError::Truncated(magic.err().unwrap()));
        }
        let magic = magic.unwrap();
        if magic != 0xCAFEBABE {
            return Err(ClassParserError::ClassParseInvalidMagic);
        }

        let minor_version = reader.u16()?;
        let major_version = reader.u16()?;

        match major_version {
            45..56 => {},
            56..70 => {
                // not currently supporting preview features
                if minor_version != 0 {
                    return Err(ClassParserError::ClassParseUnsupportedVersion);
                }
            }
            _ => {
                return Err(ClassParserError::ClassParseUnsupportedVersion);
            }
        }
        
        // Constant pool entries are version-gated but never reference the pool
        // being built, so this stands in until the real context can be made.
        let version_ctx = ClassParseCtx::new(
            major_version,
            minor_version,
            ConstantPool::new(Vec::new()),
            ClassFileAccessFlags::empty(),
            JavaUTF8(&[]),
            None,
        );

        let constant_pool = ConstantPool::parse(reader, &version_ctx)?;

        let access_flags = reader.u16()?;
        let access_flags = ClassFileAccessFlags::from_bits_truncate(access_flags);

        let this_class_index = reader.u16()?;
        let this_class_entry = constant_pool.get(this_class_index).ok_or(ClassParserError::ClassParseInvalidThisClassIndex)?;
        let this_class_name = match this_class_entry {
            ConstantPoolEntry::ClassIndex(name_index) => {
                let name_entry = constant_pool.get(*name_index).ok_or(ClassParserError::ClassParseInvalidThisClassIndex)?;
                if let ConstantPoolEntry::UTF8(name) = name_entry {
                    *name
                } else {
                    return Err(ClassParserError::ClassParseInvalidThisClassIndex);
                }
            }
            _ => return Err(ClassParserError::ClassParseInvalidThisClassIndex),
        };

        if !valid_class_name(this_class_name) {
            return Err(ClassParserError::ClassParseFormatError);
        }

        let super_class_index = reader.u16()?;
        let super_class = if super_class_index == 0 {
            None
        } else {
            let super_class_entry = constant_pool.get(super_class_index).ok_or(ClassParserError::ClassParseInvalidSuperClassIndex)?;
            let super_class_name = match super_class_entry {
                ConstantPoolEntry::ClassIndex(name_index) => {
                    let name_entry = constant_pool.get(*name_index).ok_or(ClassParserError::ClassParseInvalidSuperClassIndex)?;
                    if let ConstantPoolEntry::UTF8(name) = name_entry {
                        *name
                    } else {
                        return Err(ClassParserError::ClassParseInvalidSuperClassIndex);
                    }
                }
                _ => return Err(ClassParserError::ClassParseInvalidSuperClassIndex),
            };
            Some(super_class_name)
        };

        match super_class {
            None => {
                if !access_flags.contains(ClassFileAccessFlags::MODULE)
                    && this_class_name != "java/lang/Object"
                {
                    return Err(ClassParserError::ClassParseInvalidSuperClassIndex);
                }
            }
            Some(name) => {
                if access_flags.contains(ClassFileAccessFlags::MODULE) {
                    // The whole module-info shape is wrong, not just this index.
                    return Err(ClassParserError::ClassParseFormatError);
                }
                if access_flags.contains(ClassFileAccessFlags::INTERFACE)
                    && name != "java/lang/Object"
                {
                    return Err(ClassParserError::ClassParseInvalidSuperClassIndex);
                }
            }
        }

        let interfaces = Interfaces::parse(reader, &constant_pool)?;

        // Everything the context carries is now known, so members and
        // attributes can resolve against the real pool.
        let parser_ctx = ClassParseCtx::new(
            major_version,
            minor_version,
            constant_pool,
            access_flags,
            this_class_name,
            super_class,
        );

        let fields = Fields::parse(reader, &parser_ctx)?;
        let methods = Methods::parse(reader, &parser_ctx)?;

        let attributes = Attributes::parse(reader, &parser_ctx, AttributeLocation::ClassFile)?;

        // if reader still has bytes left, return an error
        if !reader.is_empty() {
            return Err(ClassParserError::ClassParseTrailingBytes);
        }

        // Verify access flags
        if access_flags.contains(ClassFileAccessFlags::ANNOTATION) && !access_flags.contains(ClassFileAccessFlags::INTERFACE) {
            return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
        }

        if access_flags.contains(ClassFileAccessFlags::FINAL) && access_flags.contains(ClassFileAccessFlags::ABSTRACT) {
            return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
        }

        if access_flags.contains(ClassFileAccessFlags::INTERFACE) && !access_flags.contains(ClassFileAccessFlags::ABSTRACT) {
            return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
        }

        if access_flags.contains(ClassFileAccessFlags::MODULE) {
            if access_flags.contains(ClassFileAccessFlags::FINAL)
                || access_flags.contains(ClassFileAccessFlags::SUPER)
                || access_flags.contains(ClassFileAccessFlags::INTERFACE)
                || access_flags.contains(ClassFileAccessFlags::ABSTRACT)
                || access_flags.contains(ClassFileAccessFlags::ANNOTATION)
                || access_flags.contains(ClassFileAccessFlags::ENUM)
            {
                return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
            }
        }

        if access_flags.contains(ClassFileAccessFlags::INTERFACE) {
            if access_flags.contains(ClassFileAccessFlags::ENUM)
                || access_flags.contains(ClassFileAccessFlags::FINAL)
                || access_flags.contains(ClassFileAccessFlags::SUPER)
            {
                return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
            }
        }

        return Ok(Self {
            minor_version,
            major_version,
            constant_pool: parser_ctx.pool,
            access_flags,
            this_class: this_class_name,
            super_class,
            interfaces,
            fields,
            methods,
            attributes,
        });
    }

    /// The constant at `index`, or `None` if it is out of range or names the
    /// unusable slot after a Long or Double.
    pub fn constant(&self, index: u16) -> Option<&ConstantPoolEntry<'a>> {
        self.constant_pool.get(index)
    }
}

/// The constant pool, indexed from 1 (JVMS 4.4). Dereferences to a slice, so
/// `len()` and iteration work; `get` applies the 1-based indexing and refuses
/// the unusable slot after a Long or Double.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConstantPool<'a> {
    entries: Vec<ConstantPoolEntry<'a>>,
}

impl<'a> ConstantPool<'a> {
    pub fn new(entries: Vec<ConstantPoolEntry<'a>>) -> Self {
        ConstantPool { entries }
    }

    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let constant_pool_count = reader.u16()? as usize;
        if constant_pool_count == 0 {
            return Err(ClassParserError::ClassParseInvalidConstantPool);
        }
        let mut entries: Vec<ConstantPoolEntry<'a>> = Vec::with_capacity(constant_pool_count);
        while entries.len() < constant_pool_count - 1 {
            let entry = ConstantPoolEntry::parse(reader, ctx)?;
            if matches!(entry, ConstantPoolEntry::Long(_) | ConstantPoolEntry::Double(_)) {
                entries.push(entry);
                entries.push(ConstantPoolEntry::Unusable);
            } else {
                entries.push(entry);
            }
        }
        if entries.len() != constant_pool_count - 1 {
            return Err(ClassParserError::ClassParseInvalidConstantPool);
        }
        let pool = ConstantPool { entries };
        pool.validate_references()?;
        Ok(pool)
    }

    /// The CONSTANT_NameAndType at `index`, checked for kind.
    pub fn name_and_type(&self, index: u16) -> Result<(u16, u16), ClassParserError> {
        match self.require(index)? {
            ConstantPoolEntry::NameAndType { name_index, descriptor_index } => {
                Ok((*name_index, *descriptor_index))
            }
            _ => Err(ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry),
        }
    }

    /// Checks every entry's references. This cannot happen while reading,
    /// because an entry may name one that appears later in the table
    pub fn validate_references(&self) -> Result<(), ClassParserError> {
        for entry in &self.entries {
            match entry {
                ConstantPoolEntry::ClassIndex(n) => {
                    let name = self.utf8(*n)?;
                    if !valid_class_entry_name(name) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }
                }

                // Each of these names a Utf8 as its only reference.
                ConstantPoolEntry::StringIndex(n)
                | ConstantPoolEntry::MethodTypeIndex(n)
                | ConstantPoolEntry::ModuleIndex(n)
                | ConstantPoolEntry::PackageIndex(n) => {
                    self.utf8(*n)?;
                }

                ConstantPoolEntry::NameAndType { name_index, descriptor_index } => {
                    self.utf8(*name_index)?;
                    self.utf8(*descriptor_index)?;
                }

                ConstantPoolEntry::FieldRef { class_index, name_and_type_index }
                | ConstantPoolEntry::MethodRef { class_index, name_and_type_index }
                | ConstantPoolEntry::InterfaceMethodRef { class_index, name_and_type_index } => {
                    self.class_name(*class_index)?;
                    self.name_and_type(*name_and_type_index)?;
                }

                // bootstrap_index indexes the BootstrapMethods attribute rather
                // than the pool, so it is checked once that attribute is read.
                ConstantPoolEntry::Dynamic { name_and_type_index, .. }
                | ConstantPoolEntry::InvokeDynamic { name_and_type_index, .. } => {
                    self.name_and_type(*name_and_type_index)?;
                }

                // Which kind the reference must be depends on ref_kind, so only
                // its existence is settled here (JVMS 4.4.8 has the rest).
                ConstantPoolEntry::MethodHandle { ref_index, .. } => {
                    self.require(*ref_index)?;
                }

                ConstantPoolEntry::UTF8(_)
                | ConstantPoolEntry::Integer(_)
                | ConstantPoolEntry::Float(_)
                | ConstantPoolEntry::Long(_)
                | ConstantPoolEntry::Double(_)
                | ConstantPoolEntry::Unusable => {}
            }
        }
        Ok(())
    }

    /// `constant_pool_count` as written in the file, which is one more than the
    /// number of entries.
    pub fn count(&self) -> u16 {
        self.entries.len() as u16 + 1
    }

    pub fn get(&self, index: u16) -> Option<&ConstantPoolEntry<'a>> {
        match self.entries.get(usize::from(index).checked_sub(1)?) {
            Some(ConstantPoolEntry::Unusable) | None => None,
            entry => entry,
        }
    }

    fn require(&self, index: u16) -> Result<&ConstantPoolEntry<'a>, ClassParserError> {
        self.get(index).ok_or(ClassParserError::ClassParseInvalidConstantPoolIndex)
    }

    /// The bytes of the CONSTANT_Utf8 at `index`.
    pub fn utf8(&self, index: u16) -> Result<JavaUTF8<'a>, ClassParserError> {
        match self.require(index)? {
            ConstantPoolEntry::UTF8(s) => Ok(*s),
            _ => Err(ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry),
        }
    }

    /// Like `utf8`, but index 0 means "absent" rather than being an error —
    /// the shape many optional attribute fields use.
    pub fn utf8_opt(&self, index: u16) -> Result<Option<JavaUTF8<'a>>, ClassParserError> {
        if index == 0 { Ok(None) } else { self.utf8(index).map(Some) }
    }

    /// The name of the CONSTANT_Class at `index`.
    pub fn class_name(&self, index: u16) -> Result<JavaUTF8<'a>, ClassParserError> {
        match self.require(index)? {
            ConstantPoolEntry::ClassIndex(name) => self.utf8(*name),
            _ => Err(ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry),
        }
    }

    pub fn class_name_opt(&self, index: u16) -> Result<Option<JavaUTF8<'a>>, ClassParserError> {
        if index == 0 { Ok(None) } else { self.class_name(index).map(Some) }
    }

    /// The name of the CONSTANT_Module at `index`.
    pub fn module_name(&self, index: u16) -> Result<JavaUTF8<'a>, ClassParserError> {
        match self.require(index)? {
            ConstantPoolEntry::ModuleIndex(name) => self.utf8(*name),
            _ => Err(ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry),
        }
    }

    /// The name of the CONSTANT_Package at `index`.
    pub fn package_name(&self, index: u16) -> Result<JavaUTF8<'a>, ClassParserError> {
        match self.require(index)? {
            ConstantPoolEntry::PackageIndex(name) => self.utf8(*name),
            _ => Err(ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry),
        }
    }
}

impl<'a> std::ops::Deref for ConstantPool<'a> {
    type Target = [ConstantPoolEntry<'a>];
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstantPoolEntry<'a> {
    // java uses modified UTF-8, which is not valid Rust str; see crate::java_utf
    UTF8(JavaUTF8<'a>),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ClassIndex(u16),
    StringIndex(u16),
    FieldRef { class_index: u16, name_and_type_index: u16 },
    MethodRef { class_index: u16, name_and_type_index: u16 },
    InterfaceMethodRef { class_index: u16, name_and_type_index: u16 },
    NameAndType { name_index: u16, descriptor_index: u16 },
    MethodHandle { ref_kind: MethodHandleKind, ref_index: u16 },
    MethodTypeIndex(u16),
    Dynamic { bootstrap_index: u16, name_and_type_index: u16 },
    InvokeDynamic { bootstrap_index: u16, name_and_type_index: u16 },
    ModuleIndex(u16),
    PackageIndex(u16),
    Unusable, // The space after the 64 bit entries long / double
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodHandleKind {
    GetField = 1,
    GetStatic = 2,
    PutField = 3,
    PutStatic = 4,
    InvokeVirtual = 5,
    InvokeStatic = 6,
    InvokeSpecial = 7,
    NewInvokeSpecial = 8,
    InvokeInterface = 9,
}

impl<'a> ConstantPoolEntry<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let tag = reader.u8()?;
        match tag {
            1 => {
                let length = reader.u16()? as usize;
                let java_string = JavaUTF8(reader.bytes(length)?);
                if !java_string.is_valid() {
                    return Err(ClassParserError::InvalidUTF8(JavaUTF8Error::InvalidSequence));
                }
                return Ok(ConstantPoolEntry::UTF8(java_string))
            },
            3 => { return Ok(ConstantPoolEntry::Integer(reader.i32()?)); },
            4 => { return Ok(ConstantPoolEntry::Float(reader.f32()?)); },
            5 => { return Ok(ConstantPoolEntry::Long(reader.i64()?)); },
            6 => { return Ok(ConstantPoolEntry::Double(reader.f64()?)); },
            7 => { return Ok(ConstantPoolEntry::ClassIndex(reader.u16()?)); },
            8 => { return Ok(ConstantPoolEntry::StringIndex(reader.u16()?)); },
            9 => {
                let class_index = reader.u16()?;
                let name_and_type_index = reader.u16()?;
                return Ok(ConstantPoolEntry::FieldRef { class_index, name_and_type_index });
            },
            10 => {
                let class_index = reader.u16()?;
                let name_and_type_index = reader.u16()?;
                return Ok(ConstantPoolEntry::MethodRef { class_index, name_and_type_index });
            },
            11 => {
                let class_index = reader.u16()?;
                let name_and_type_index = reader.u16()?;
                return Ok(ConstantPoolEntry::InterfaceMethodRef { class_index, name_and_type_index });
            },
            12 => {
                let name_index = reader.u16()?;
                let descriptor_index = reader.u16()?;
                return Ok(ConstantPoolEntry::NameAndType { name_index, descriptor_index });
            },
            15 => {
                if !ctx.at_least(51) {
                    return Err(ClassParserError::ClassParseInvalidFeatureUsedForVersion);
                }
                let ref_kind = reader.u8()?;
                let ref_kind = match ref_kind {
                    1 => MethodHandleKind::GetField,
                    2 => MethodHandleKind::GetStatic,
                    3 => MethodHandleKind::PutField,
                    4 => MethodHandleKind::PutStatic,
                    5 => MethodHandleKind::InvokeVirtual,
                    6 => MethodHandleKind::InvokeStatic,
                    7 => MethodHandleKind::InvokeSpecial,
                    8 => MethodHandleKind::NewInvokeSpecial,
                    9 => MethodHandleKind::InvokeInterface,
                    _ => return Err(ClassParserError::ClassParseFormatError),
                };
                let ref_index = reader.u16()?;
                return Ok(ConstantPoolEntry::MethodHandle { ref_kind, ref_index });
            },
            16 => { 
                if !ctx.at_least(51) {
                    return Err(ClassParserError::ClassParseInvalidFeatureUsedForVersion);
                }
                return Ok(ConstantPoolEntry::MethodTypeIndex(reader.u16()?)); 
            },
            17 => {
                if !ctx.at_least(55) {
                    return Err(ClassParserError::ClassParseInvalidFeatureUsedForVersion);
                }
                let bootstrap_index = reader.u16()?;
                let name_and_type_index = reader.u16()?;
                return Ok(ConstantPoolEntry::Dynamic { bootstrap_index, name_and_type_index });
            },
            18 => {
                if !ctx.at_least(51) {
                    return Err(ClassParserError::ClassParseInvalidFeatureUsedForVersion);
                }
                let bootstrap_index = reader.u16()?;
                let name_and_type_index = reader.u16()?;
                return Ok(ConstantPoolEntry::InvokeDynamic { bootstrap_index, name_and_type_index });
            },
            19 => { return Ok(ConstantPoolEntry::ModuleIndex(reader.u16()?)); },
            20 => { return Ok(ConstantPoolEntry::PackageIndex(reader.u16()?)); },
            _ => { return Err(ClassParserError::ClassParseInvalidConstantPoolTag) }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo<'a> {
    pub access_flags: FieldInfoAccessFlags,
    pub name: JavaUTF8<'a>,
    pub descriptor: JavaUTF8<'a>,

    pub attributes: Attributes<'a>,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FieldInfoAccessFlags: u16 {
        const PUBLIC = 0x0001;
        const PRIVATE = 0x0002;
        const PROTECTED = 0x0004;
        const STATIC = 0x0008;
        const FINAL = 0x0010;
        const VOLATILE = 0x0040;
        const TRANSIENT = 0x0080;
        const SYNTHETIC = 0x1000;
        const ENUM = 0x4000;
    }
}

impl<'a> FieldInfo<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let access_flags = reader.u16()?;
        let access_flags = FieldInfoAccessFlags::from_bits_truncate(access_flags);

        let name_index = reader.u16()?;
        let name = ctx.pool.utf8(name_index)?;

        let descriptor_index = reader.u16()?;
        let descriptor = ctx.pool.utf8(descriptor_index)?;

        let attributes = Attributes::parse(reader, ctx, AttributeLocation::Field)?;

        return Ok(Self {
            access_flags,
            name,
            descriptor,
            attributes,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodInfo<'a> {
    pub access_flags: MethodInfoAccessFlags,
    pub name: JavaUTF8<'a>,
    pub descriptor: JavaUTF8<'a>,

    pub attributes: Attributes<'a>,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MethodInfoAccessFlags: u16 {
        const PUBLIC = 0x0001;
        const PRIVATE = 0x0002;
        const PROTECTED = 0x0004;
        const STATIC = 0x0008;
        const FINAL = 0x0010;
        const SYNCHRONIZED = 0x0020;
        const BRIDGE = 0x0040;
        const VARARGS = 0x0080;
        const NATIVE = 0x0100;
        const ABSTRACT = 0x0400;
        const STRICT = 0x0800;
        const SYNTHETIC = 0x1000;
    }
}

impl<'a> MethodInfo<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let access_flags = reader.u16()?;
        let access_flags = MethodInfoAccessFlags::from_bits_truncate(access_flags);

        let name_index = reader.u16()?;
        let name = ctx.pool.utf8(name_index)?;

        let descriptor_index = reader.u16()?;
        let descriptor = ctx.pool.utf8(descriptor_index)?;

        let attributes = Attributes::parse(reader, ctx, AttributeLocation::Method)?;

        return Ok(Self {
            access_flags,
            name,
            descriptor,
            attributes,
        });
    }

}

#[derive(Debug, Clone, PartialEq)]
pub enum AttributeInfo<'a> {
    ConstantValueIndex {
        value: u16,
    },
    Code {
        max_stack: u16,
        max_locals: u16,

        code: &'a [u8],

        exception_table: Vec<ExceptionTableEntry<'a>>,

        attributes: Attributes<'a>,
    },
    StackMapTable {
        entries: Vec<StackMapFrame>,
    },
    Exceptions {
        entries: Vec<JavaUTF8<'a>>,
    },
    InnerClasses {
        value: Vec<InnerClassInfo<'a>>,
    },
    EnclosingMethod {
        enclosing_class: JavaUTF8<'a>,
        /// A CONSTANT_NameAndType index, or `None` when the class is not
        /// enclosed by a method (JVMS 4.7.7).
        enclosing_method: Option<u16>,
    },
    Synthetic,
    Signature {
        value: JavaUTF8<'a>,
    },
    SourceFileIndex {
        value: JavaUTF8<'a>,
    },
    SourceDebugExtension {
        value: &'a [u8],
    },
    LineNumberTable {
        entries: Vec<LineNumberTableEntry>,
    },
    LocalVariableTable {
        entries: Vec<LocalVariableTableEntry<'a>>,
    },
    LocalVariableTypeTable {
        entries: Vec<LocalVariableTypeTableEntry<'a>>,
    },
    Deprecated,
    RuntimeVisibleAnnotations {
        value: Vec<Annotation<'a>>,
    },
    RuntimeInvisibleAnnotations {
        value: Vec<Annotation<'a>>,
    },
    RuntimeVisibleParameterAnnotations {
        value: Vec<ParameterAnnotation<'a>>,
    },
    RuntimeInvisibleParameterAnnotations {
        value: Vec<ParameterAnnotation<'a>>,
    },
    RuntimeVisibleTypeAnnotations {
        value: Vec<TypeAnnotation<'a>>,
    },
    RuntimeInvisibleTypeAnnotations {
        value: Vec<TypeAnnotation<'a>>,
    },
    AnnotationDefault {
        value: ElementValue<'a>,
    },
    BootstrapMethods {
        value: Vec<BootstrapMethod>,
    },
    MethodParameters {
        value: Vec<MethodParameter<'a>>,
    },
    Module {
        value: ModuleAttribute<'a>,
    },
    ModulePackages {
        value: Vec<JavaUTF8<'a>>,
    },
    ModuleMainClass {
        value: JavaUTF8<'a>,
    },
    NestHostClass {
        value: JavaUTF8<'a>,
    },
    NestMembers {
        value: Vec<JavaUTF8<'a>>,
    },
    Record {
        value: RecordAttribute<'a>,
    },
    PermittedSubclasses {
        value: Vec<JavaUTF8<'a>>,
    },
    /// An attribute this parser doesn't recognise. JVMS 4.7.1 requires these
    /// to be skipped rather than rejected, so the raw bytes are kept as-is.
    Unknown {
        name: JavaUTF8<'a>,
        info: &'a [u8],
    },
}

/// The tables inside a Module attribute (JVMS 4.7.25). Each forbids naming the
/// same module, package or service twice.
macro_rules! module_table {
    ($name:ident, $item:ty, $key:expr, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Default)]
        pub struct $name<'a>(Vec<$item>);

        impl<'a> $name<'a> {
            fn parse(
                reader: &mut Reader<'a>,
                ctx: &ClassParseCtx<'a>,
            ) -> Result<Self, ClassParserError> {
                let count = reader.u16()? as usize;
                let mut items: Vec<$item> = Vec::with_capacity(count);
                for _ in 0..count {
                    let entry = <$item>::parse(reader, ctx)?;
                    let key = $key;
                    if items.iter().any(|seen| key(seen) == key(&entry)) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }
                    items.push(entry);
                }
                Ok($name(items))
            }
        }

        impl<'a> std::ops::Deref for $name<'a> {
            type Target = [$item];
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl<'s, 'a> IntoIterator for &'s $name<'a> {
            type Item = &'s $item;
            type IntoIter = std::slice::Iter<'s, $item>;
            fn into_iter(self) -> Self::IntoIter {
                self.0.iter()
            }
        }
    };
}

module_table!(
    ModuleRequires,
    ModuleRequiresEntry<'a>,
    |e: &ModuleRequiresEntry<'a>| e.requires,
    "The modules this one depends on; each may be named once."
);
module_table!(
    ModuleExports,
    ModuleExportsEntry<'a>,
    |e: &ModuleExportsEntry<'a>| e.exports,
    "The packages this module exports; each may be named once."
);
module_table!(
    ModuleOpens,
    ModuleOpensEntry<'a>,
    |e: &ModuleOpensEntry<'a>| e.opens,
    "The packages this module opens; each may be named once."
);
module_table!(
    ModuleProvides,
    ModuleProvidesEntry<'a>,
    |e: &ModuleProvidesEntry<'a>| e.provides,
    "The services this module implements; each may be named once."
);

/// The services this module consumes; each may be named once.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModuleUses<'a>(Vec<JavaUTF8<'a>>);

impl<'a> ModuleUses<'a> {
    fn parse(
        reader: &mut Reader<'a>,
        ctx: &ClassParseCtx<'a>,
    ) -> Result<Self, ClassParserError> {
        let count = reader.u16()? as usize;
        let mut items: Vec<JavaUTF8<'a>> = Vec::with_capacity(count);
        for _ in 0..count {
            let name = ctx.pool.class_name(reader.u16()?)?;
            if items.contains(&name) {
                return Err(ClassParserError::ClassParseFormatError);
            }
            items.push(name);
        }
        Ok(ModuleUses(items))
    }
}

impl<'a> std::ops::Deref for ModuleUses<'a> {
    type Target = [JavaUTF8<'a>];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'s, 'a> IntoIterator for &'s ModuleUses<'a> {
    type Item = &'s JavaUTF8<'a>;
    type IntoIter = std::slice::Iter<'s, JavaUTF8<'a>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// The interfaces a class directly implements. JVMS 4.1 does not allow the
/// same interface twice.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Interfaces<'a>(Vec<JavaUTF8<'a>>);

impl<'a> Interfaces<'a> {
    pub fn parse(
        reader: &mut Reader<'a>,
        pool: &ConstantPool<'a>,
    ) -> Result<Self, ClassParserError> {
        let count = reader.u16()? as usize;
        let mut items: Vec<JavaUTF8<'a>> = Vec::with_capacity(count);
        for _ in 0..count {
            let name = pool.class_name(reader.u16()?)?;
            if items.contains(&name) {
                return Err(ClassParserError::ClassParseFormatError);
            }
            items.push(name);
        }
        Ok(Interfaces(items))
    }
}

/// A class's fields. No two may share both a name and a descriptor (JVMS 4.5).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fields<'a>(Vec<FieldInfo<'a>>);

impl<'a> Fields<'a> {
    pub fn parse(
        reader: &mut Reader<'a>,
        ctx: &ClassParseCtx<'a>,
    ) -> Result<Self, ClassParserError> {
        let count = reader.u16()? as usize;
        let mut items: Vec<FieldInfo<'a>> = Vec::with_capacity(count);
        for _ in 0..count {
            let field = FieldInfo::parse(reader, ctx)?;
            if items
                .iter()
                .any(|seen| seen.name == field.name && seen.descriptor == field.descriptor)
            {
                return Err(ClassParserError::ClassParseFormatError);
            }
            items.push(field);
        }
        Ok(Fields(items))
    }
}

/// A class's methods. No two may share both a name and a descriptor (JVMS 4.6),
/// though differing only in return type is fine.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Methods<'a>(Vec<MethodInfo<'a>>);

impl<'a> Methods<'a> {
    pub fn parse(
        reader: &mut Reader<'a>,
        ctx: &ClassParseCtx<'a>,
    ) -> Result<Self, ClassParserError> {
        let count = reader.u16()? as usize;
        let mut items: Vec<MethodInfo<'a>> = Vec::with_capacity(count);
        for _ in 0..count {
            let method = MethodInfo::parse(reader, ctx)?;
            if items
                .iter()
                .any(|seen| seen.name == method.name && seen.descriptor == method.descriptor)
            {
                return Err(ClassParserError::ClassParseFormatError);
            }
            items.push(method);
        }
        Ok(Methods(items))
    }
}

impl<'a> std::ops::Deref for Interfaces<'a> {
    type Target = [JavaUTF8<'a>];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'s, 'a> IntoIterator for &'s Interfaces<'a> {
    type Item = &'s JavaUTF8<'a>;
    type IntoIter = std::slice::Iter<'s, JavaUTF8<'a>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> std::ops::Deref for Fields<'a> {
    type Target = [FieldInfo<'a>];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'s, 'a> IntoIterator for &'s Fields<'a> {
    type Item = &'s FieldInfo<'a>;
    type IntoIter = std::slice::Iter<'s, FieldInfo<'a>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> std::ops::Deref for Methods<'a> {
    type Target = [MethodInfo<'a>];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'s, 'a> IntoIterator for &'s Methods<'a> {
    type Item = &'s MethodInfo<'a>;
    type IntoIter = std::slice::Iter<'s, MethodInfo<'a>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Where an attributes table sits, which decides both what may appear in it
/// (JVMS table 4.7-C) and whether a type annotation's target is legal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeLocation {
    ClassFile,
    Field,
    Method,
    Code,
    RecordComponent,
}

/// An attributes table. Parsing and the rules that need the whole table — what
/// may appear here, and what may appear only once — live together.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attributes<'a>(Vec<AttributeInfo<'a>>);

impl<'a> Attributes<'a> {
    pub fn parse(
        reader: &mut Reader<'a>,
        ctx: &ClassParseCtx<'a>,
        location: AttributeLocation,
    ) -> Result<Self, ClassParserError> {
        let count = reader.u16()? as usize;
        let mut items: Vec<AttributeInfo<'a>> = Vec::with_capacity(count);
        for _ in 0..count {
            let attribute = AttributeInfo::parse(reader, ctx)?;
            if !attribute.may_appear_at(location) {
                return Err(ClassParserError::ClassParseFormatError);
            }
            if attribute.at_most_once()
                && items.iter().any(|seen| seen.kind() == attribute.kind())
            {
                return Err(ClassParserError::ClassParseFormatError);
            }
            items.push(attribute);
        }
        Ok(Attributes(items))
    }

    pub fn into_vec(self) -> Vec<AttributeInfo<'a>> {
        self.0
    }
}

impl<'a> std::ops::Deref for Attributes<'a> {
    type Target = [AttributeInfo<'a>];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'s, 'a> IntoIterator for &'s Attributes<'a> {
    type Item = &'s AttributeInfo<'a>;
    type IntoIter = std::slice::Iter<'s, AttributeInfo<'a>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> AttributeInfo<'a> {
    /// Discriminant only, for the "at most one of these" rule.
    fn kind(&self) -> std::mem::Discriminant<AttributeInfo<'a>> {
        std::mem::discriminant(self)
    }

    fn may_appear_at(&self, at: AttributeLocation) -> bool {
        use AttributeInfo as A;
        use AttributeLocation as L;
        match self {
            // An unrecognised attribute may appear anywhere and is skipped.
            A::Unknown { .. } => true,

            A::ConstantValueIndex { .. } => at == L::Field,

            A::Code { .. }
            | A::Exceptions { .. }
            | A::RuntimeVisibleParameterAnnotations { .. }
            | A::RuntimeInvisibleParameterAnnotations { .. }
            | A::AnnotationDefault { .. }
            | A::MethodParameters { .. } => at == L::Method,

            A::StackMapTable { .. }
            | A::LineNumberTable { .. }
            | A::LocalVariableTable { .. }
            | A::LocalVariableTypeTable { .. } => at == L::Code,

            A::InnerClasses { .. }
            | A::EnclosingMethod { .. }
            | A::SourceFileIndex { .. }
            | A::SourceDebugExtension { .. }
            | A::BootstrapMethods { .. }
            | A::Module { .. }
            | A::ModulePackages { .. }
            | A::ModuleMainClass { .. }
            | A::NestHostClass { .. }
            | A::NestMembers { .. }
            | A::Record { .. }
            | A::PermittedSubclasses { .. } => at == L::ClassFile,

            A::Synthetic | A::Deprecated => {
                matches!(at, L::ClassFile | L::Field | L::Method)
            }

            A::Signature { .. } => {
                matches!(at, L::ClassFile | L::Field | L::Method | L::RecordComponent)
            }

            A::RuntimeVisibleAnnotations { .. } | A::RuntimeInvisibleAnnotations { .. } => {
                matches!(at, L::ClassFile | L::Field | L::Method | L::RecordComponent)
            }

            A::RuntimeVisibleTypeAnnotations { .. }
            | A::RuntimeInvisibleTypeAnnotations { .. } => true,
        }
    }

    fn at_most_once(&self) -> bool {
        !matches!(
            self,
            AttributeInfo::LineNumberTable { .. }
                | AttributeInfo::LocalVariableTable { .. }
                | AttributeInfo::LocalVariableTypeTable { .. }
                | AttributeInfo::Unknown { .. }
        )
    }

    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let name_index = reader.u16()?;
        let name = ctx.pool.utf8(name_index)?;

        let attribute_length = reader.u32()? as usize;
        // Short here means the file ended; short inside the body means the
        // declared length disagrees with the contents, which parse_body maps.
        let info = reader.bytes(attribute_length)?;

        Self::parse_body(name, info, ctx).map_err(|e| match e {
            ClassParserError::Truncated(_) => ClassParserError::ClassParseInvalidAttributeLength,
            other => other,
        })
    }

    fn parse_body(
        name: JavaUTF8<'a>,
        info: &'a [u8],
        ctx: &ClassParseCtx<'a>,
    ) -> Result<Self, ClassParserError> {
        let mut info_reader = Reader::new(info);

        match name.as_bytes() {
            b"ConstantValue" => {
                let value = info_reader.u16()?;
                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }
                return Ok(AttributeInfo::ConstantValueIndex { value });
            },
            b"Code" => {
                let max_stack = info_reader.u16()?;
                let max_locals = info_reader.u16()?;

                let code_length = info_reader.u32()? as usize;
                if code_length >= 65536 || code_length == 0 {
                    return Err(ClassParserError::ClassParseInvalidCodeAttribute);
                }
                let code = info_reader.bytes(code_length)?;

                let exception_table_length = info_reader.u16()? as usize;
                let mut exception_table = Vec::with_capacity(exception_table_length);
                for _ in 0..exception_table_length {
                    let entry = ExceptionTableEntry::parse(&mut info_reader, ctx)?;
                    exception_table.push(entry);
                }

                let attributes = Attributes::parse(&mut info_reader, ctx, AttributeLocation::Code)?;

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                // at most one of the attributes can be a StackMapTable
                let stack_map_table_count = attributes.iter().filter(|attr| matches!(attr, AttributeInfo::StackMapTable { .. })).count();
                if stack_map_table_count > 1 {
                    return Err(ClassParserError::ClassParseFormatError);
                }

                return Ok(AttributeInfo::Code {
                    max_stack,
                    max_locals,
                    code,
                    exception_table,
                    attributes,
                });
            },
            b"StackMapTable" => {
                let entries_count = info_reader.u16()? as usize;
                let mut entries = Vec::with_capacity(entries_count);
                for _ in 0..entries_count {
                    let entry = StackMapFrame::parse(&mut info_reader, ctx)?;
                    entries.push(entry);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::StackMapTable { entries });
            },
            b"Exceptions" => {
                let entries_count = info_reader.u16()? as usize;
                let mut entries = Vec::with_capacity(entries_count);
                for _ in 0..entries_count {
                    let entry_index = info_reader.u16()?;
                    let entry_name = ctx.pool.class_name(entry_index)?;
                    entries.push(entry_name);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::Exceptions { entries });
            },
            b"InnerClasses" => {
                let entries_count = info_reader.u16()? as usize;
                let mut entries = Vec::with_capacity(entries_count);
                for _ in 0..entries_count {
                    let inner_class_index = info_reader.u16()?;
                    let inner_class_name = ctx.pool.class_name(inner_class_index)?;

                    let outer_class_index = info_reader.u16()?;
                    let outer_class_name = if outer_class_index == 0 {
                        None
                    } else {
                        Some(ctx.pool.class_name(outer_class_index)?)
                    };

                    let inner_name_index = info_reader.u16()?;
                    let inner_name = if inner_name_index == 0 {
                        None
                    } else {
                        Some(ctx.pool.utf8(inner_name_index)?)
                    };

                    let access_flags = info_reader.u16()?;
                    let access_flags = InnerClassAccessFlags::from_bits_truncate(access_flags);

                    entries.push(InnerClassInfo {
                        inner_class: inner_class_name,
                        outer_class: outer_class_name,
                        inner_name,
                        access_flags,
                    });
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::InnerClasses { value: entries });
            },
            b"EnclosingMethod" => {
                let enclosing_class_index = info_reader.u16()?;
                let enclosing_class_name = ctx.pool.class_name(enclosing_class_index)?;

                let enclosing_method_index = info_reader.u16()?;
                let enclosing_method = if enclosing_method_index == 0 {
                    None
                } else {
                    Some(enclosing_method_index)
                };

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::EnclosingMethod {
                    enclosing_class: enclosing_class_name,
                    enclosing_method,
                });
            },
            b"Synthetic" => {
                return if info_reader.remaining() != 0 {
                    Err(ClassParserError::ClassParseInvalidAttributeLength)
                } else {
                    Ok(AttributeInfo::Synthetic)
                };
            },
            b"Signature" => {
                let signature_index = info_reader.u16()?;
                let signature = ctx.pool.utf8(signature_index)?;

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::Signature { value: signature });
            },
            b"SourceFile" => {
                let source_file_index = info_reader.u16()?;
                let source_file = ctx.pool.utf8(source_file_index)?;

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::SourceFileIndex { value: source_file });
            },
            b"SourceDebugExtension" => {
                let value = info_reader.bytes(info_reader.remaining())?;

                return Ok(AttributeInfo::SourceDebugExtension { value });
            },
            b"LineNumberTable" => {
                let entries_count = info_reader.u16()? as usize;
                let mut entries = Vec::with_capacity(entries_count);
                for _ in 0..entries_count {
                    let entry = LineNumberTableEntry::parse(&mut info_reader)?;
                    entries.push(entry);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::LineNumberTable { entries });
            },
            b"LocalVariableTable" => {
                let entries_count = info_reader.u16()? as usize;
                let mut entries = Vec::with_capacity(entries_count);
                for _ in 0..entries_count {
                    let entry = LocalVariableTableEntry::parse(&mut info_reader, ctx)?;
                    entries.push(entry);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::LocalVariableTable { entries });
            },
            b"LocalVariableTypeTable" => {
                let entries_count = info_reader.u16()? as usize;
                let mut entries = Vec::with_capacity(entries_count);
                for _ in 0..entries_count {
                    let entry = LocalVariableTypeTableEntry::parse(&mut info_reader, ctx)?;
                    entries.push(entry);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::LocalVariableTypeTable { entries });
            },
            b"Deprecated" => {
                return if info_reader.remaining() != 0 {
                    Err(ClassParserError::ClassParseInvalidAttributeLength)
                } else {
                    Ok(AttributeInfo::Deprecated)
                };
            },
            b"RuntimeVisibleAnnotations" => {
                let count = info_reader.u16()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let annotation = Annotation::parse(&mut info_reader, ctx)?;
                    value.push(annotation);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::RuntimeVisibleAnnotations { value });
            },
            b"RuntimeInvisibleAnnotations" => {
                let count = info_reader.u16()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let annotation = Annotation::parse(&mut info_reader, ctx)?;
                    value.push(annotation);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::RuntimeInvisibleAnnotations { value });
            },
            b"RuntimeVisibleParameterAnnotations" => {
                let count = info_reader.u8()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let parameter_annotation = ParameterAnnotation::parse(&mut info_reader, ctx)?;
                    value.push(parameter_annotation);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::RuntimeVisibleParameterAnnotations { value });
            },
            b"RuntimeInvisibleParameterAnnotations" => {
                let count = info_reader.u8()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let parameter_annotation = ParameterAnnotation::parse(&mut info_reader, ctx)?;
                    value.push(parameter_annotation);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::RuntimeInvisibleParameterAnnotations { value });
            },
            b"RuntimeVisibleTypeAnnotations" => {
                let count = info_reader.u16()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let type_annotation = TypeAnnotation::parse(&mut info_reader, ctx)?;
                    value.push(type_annotation);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::RuntimeVisibleTypeAnnotations { value });
            },
            b"RuntimeInvisibleTypeAnnotations" => {
                let count = info_reader.u16()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let type_annotation = TypeAnnotation::parse(&mut info_reader, ctx)?;
                    value.push(type_annotation);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::RuntimeInvisibleTypeAnnotations { value });
            },
            b"AnnotationDefault" => {
                let value = ElementValue::parse(&mut info_reader, ctx)?;

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::AnnotationDefault { value });
            },
            b"BootstrapMethods" => {
                let count = info_reader.u16()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let bootstrap_method = BootstrapMethod::parse(&mut info_reader, ctx)?;
                    value.push(bootstrap_method);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::BootstrapMethods { value });
            },
            b"MethodParameters" => {
                let count = info_reader.u8()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let method_parameter = MethodParameter::parse(&mut info_reader, ctx)?;
                    value.push(method_parameter);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::MethodParameters { value });
            },
            b"Module" => {
                if !ctx.access_flags.contains(ClassFileAccessFlags::MODULE) {
                    return Err(ClassParserError::ClassParseInvalidFeatureUsedForVersion);
                }
                let value = ModuleAttribute::parse(&mut info_reader, ctx)?;

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::Module { value });
            },
            b"ModulePackages" => {
                let count = info_reader.u16()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let package_index = info_reader.u16()?;
                    let package_name = ctx.pool.package_name(package_index)?;
                    value.push(package_name);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::ModulePackages { value });
            },
            b"ModuleMainClass" => {
                let main_class_index = info_reader.u16()?;
                let main_class_name = ctx.pool.class_name(main_class_index)?;

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::ModuleMainClass { value: main_class_name });
            },
            b"NestHost" => {
                let host_class_index = info_reader.u16()?;
                let host_class_name = ctx.pool.class_name(host_class_index)?;

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::NestHostClass { value: host_class_name });
            }
            b"NestMembers" => {
                let count = info_reader.u16()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let class_index = info_reader.u16()?;
                    let class_name = ctx.pool.class_name(class_index)?;
                    value.push(class_name);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::NestMembers { value });
            }
            b"Record" => {
                let value = RecordAttribute::parse(&mut info_reader, ctx)?;

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::Record { value });
            }
            b"PermittedSubclasses" => {
                let count = info_reader.u16()? as usize;
                let mut value = Vec::with_capacity(count);
                for _ in 0..count {
                    let class_index = info_reader.u16()?;
                    let class_name = ctx.pool.class_name(class_index)?;
                    value.push(class_name);
                }

                if info_reader.remaining() != 0 {
                    return Err(ClassParserError::ClassParseInvalidAttributeLength);
                }

                return Ok(AttributeInfo::PermittedSubclasses { value });
            }
            _ => {
                // Unknown attribute, just store the raw bytes
                return Ok(AttributeInfo::Unknown { name, info });
            }

        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExceptionTableEntry<'a> {
    pub start_pc: u16,
    pub end_pc: u16,
    pub handler_pc: u16,
    /// `None` means "any", used for the finally handler (JVMS 4.7.3).
    pub catch_type: Option<JavaUTF8<'a>>,
}

impl<'a> ExceptionTableEntry<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let start_pc = reader.u16()?;
        let end_pc = reader.u16()?;
        let handler_pc = reader.u16()?;

        let catch_type_index = reader.u16()?;
        let catch_type = if catch_type_index == 0 {
            None
        } else {
            Some(ctx.pool.class_name(catch_type_index)?)
        };

        return Ok(Self {
            start_pc,
            end_pc,
            handler_pc,
            catch_type,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineNumberTableEntry {
    pub start_pc: u16,
    pub line_number: u16,
}

impl LineNumberTableEntry {
    pub fn parse(reader: &mut Reader) -> Result<Self, ClassParserError> {
        let start_pc = reader.u16()?;
        let line_number = reader.u16()?;

        return Ok(Self {
            start_pc,
            line_number,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalVariableTableEntry<'a> {
    pub start_pc: u16,
    pub length: u16,
    pub name: JavaUTF8<'a>,
    pub descriptor: JavaUTF8<'a>,
    pub slot: u16,
}

impl<'a> LocalVariableTableEntry<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let start_pc = reader.u16()?;

        let length = reader.u16()?;

        let name_index = reader.u16()?;
        let name = ctx.pool.utf8(name_index)?;

        let descriptor_index = reader.u16()?;
        let descriptor = ctx.pool.utf8(descriptor_index)?;

        let slot = reader.u16()?;

        return Ok(Self {
            start_pc,
            length,
            name,
            descriptor,
            slot,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalVariableTypeTableEntry<'a> {
    pub start_pc: u16,
    pub length: u16,
    pub name: JavaUTF8<'a>,
    pub signature: JavaUTF8<'a>,
    pub slot: u16,
}

impl<'a> LocalVariableTypeTableEntry<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let start_pc = reader.u16()?;

        let length = reader.u16()?;

        let name_index = reader.u16()?;
        let name = ctx.pool.utf8(name_index)?;

        let signature_index = reader.u16()?;
        let signature = ctx.pool.utf8(signature_index)?;

        let slot = reader.u16()?;

        return Ok(Self {
            start_pc,
            length,
            name,
            signature,
            slot,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StackMapFrame {
    SameFrame { frame_type: u8 },
    SameLocals1StackItemFrame { frame_type: u8, stack: VerificationTypeInfo },
    SameLocals1StackItemFrameExtended { frame_type: u8, offset_delta: u16, stack: VerificationTypeInfo },
    ChopFrame { frame_type: u8, offset_delta: u16 },
    SameFrameExtended { frame_type: u8, offset_delta: u16 },
    AppendFrame { frame_type: u8, offset_delta: u16, locals: Vec<VerificationTypeInfo> },
    FullFrame {
        frame_type: u8,
        offset_delta: u16,
        locals: Vec<VerificationTypeInfo>,
        stack: Vec<VerificationTypeInfo>,
    },
}

impl StackMapFrame {
    pub fn parse<'a>(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let frame_type = reader.u8()?;
        match frame_type {
            0..=63 => Ok(StackMapFrame::SameFrame { frame_type }),
            64..=127 => {
                let stack = VerificationTypeInfo::parse(reader, ctx)?;
                Ok(StackMapFrame::SameLocals1StackItemFrame { frame_type, stack })
            },
            247 => {
                let offset_delta = reader.u16()?;
                let stack = VerificationTypeInfo::parse(reader, ctx)?;
                Ok(StackMapFrame::SameLocals1StackItemFrameExtended { frame_type, offset_delta, stack })
            },
            248..=250 => {
                let offset_delta = reader.u16()?;
                Ok(StackMapFrame::ChopFrame { frame_type, offset_delta })
            },
            251 => {
                let offset_delta = reader.u16()?;
                Ok(StackMapFrame::SameFrameExtended { frame_type, offset_delta })
            },
            252..=254 => {
                let offset_delta = reader.u16()?;
                let locals_count = (frame_type - 251) as usize;
                let mut locals = Vec::with_capacity(locals_count);
                for _ in 0..locals_count {
                    let local = VerificationTypeInfo::parse(reader, ctx)?;
                    locals.push(local);
                }
                Ok(StackMapFrame::AppendFrame { frame_type, offset_delta, locals })
            },
            255 => {
                let offset_delta = reader.u16()?;
                let locals_count = reader.u16()? as usize;
                let mut locals = Vec::with_capacity(locals_count);
                for _ in 0..locals_count {
                    let local = VerificationTypeInfo::parse(reader, ctx)?;
                    locals.push(local);
                }
                let stack_count = reader.u16()? as usize;
                let mut stack = Vec::with_capacity(stack_count);
                for _ in 0..stack_count {
                    let stack_item = VerificationTypeInfo::parse(reader, ctx)?;
                    stack.push(stack_item);
                }
                Ok(StackMapFrame::FullFrame { frame_type, offset_delta, locals, stack })
            },
            _ => Err(ClassParserError::ClassParseInvalidStackMapFrameType),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VerificationTypeInfo {
    TopVariableInfo,
    IntegerVariableInfo,
    FloatVariableInfo,
    LongVariableInfo,
    DoubleVariableInfo,
    NullVariableInfo,
    UninitializedThisVariableInfo,
    ObjectVariableInfo { cpool_index: u16 },
    UninitializedVariableInfo { offset: u16 },
}

impl VerificationTypeInfo {
    pub fn parse<'a>(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let tag = reader.u8()?;
        match tag {
            0 => Ok(VerificationTypeInfo::TopVariableInfo),
            1 => Ok(VerificationTypeInfo::IntegerVariableInfo),
            2 => Ok(VerificationTypeInfo::FloatVariableInfo),
            3 => Ok(VerificationTypeInfo::DoubleVariableInfo),
            4 => Ok(VerificationTypeInfo::LongVariableInfo),
            5 => Ok(VerificationTypeInfo::NullVariableInfo),
            6 => Ok(VerificationTypeInfo::UninitializedThisVariableInfo),
            7 => {
                let cpool_index = reader.u16()?;
                if cpool_index == 0 || cpool_index > ctx.pool.len() as u16 {
                    return Err(ClassParserError::ClassParseInvalidConstantPoolIndex);
                }
                match ctx.pool.get(cpool_index) {
                    Some(ConstantPoolEntry::ClassIndex(_)) => {},
                    None => {
                        return Err(ClassParserError::ClassParseInvalidConstantPoolIndex);
                    },
                    _ => return Err(ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry),
                }
                Ok(VerificationTypeInfo::ObjectVariableInfo { cpool_index })
            },
            8 => {
                let offset = reader.u16()?;
                Ok(VerificationTypeInfo::UninitializedVariableInfo { offset })
            },
            _ => Err(ClassParserError::ClassParseInvalidVerificationTypeTag),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InnerClassInfo<'a> {
    pub inner_class: JavaUTF8<'a>,
    /// `None` for a local or anonymous class (JVMS 4.7.6).
    pub outer_class: Option<JavaUTF8<'a>>,
    /// `None` for an anonymous class.
    pub inner_name: Option<JavaUTF8<'a>>,
    pub access_flags: InnerClassAccessFlags,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct InnerClassAccessFlags: u16 {
        const PUBLIC = 0x0001;
        const PRIVATE = 0x0002;
        const PROTECTED = 0x0004;
        const STATIC = 0x0008;
        const FINAL = 0x0010;
        const INTERFACE = 0x0200;
        const ABSTRACT = 0x0400;
        const SYNTHETIC = 0x1000;
        const ANNOTATION = 0x2000;
        const ENUM = 0x4000;
    }
}

impl<'a> InnerClassInfo<'a> {
    pub fn parse(reader: &mut Reader<'a>, pool: &ConstantPool<'a>) -> Result<Self, ClassParserError> {
        let inner_class_index = reader.u16()?;
        let inner_class = pool.class_name(inner_class_index)?;

        let outer_class_index = reader.u16()?;
        let outer_class = if outer_class_index == 0 {
            None
        } else {
            Some(pool.class_name(outer_class_index)?)
        };

        let inner_name_index = reader.u16()?;
        let inner_name = if inner_name_index == 0 {
            None
        } else {
            Some(pool.utf8(inner_name_index)?)
        };

        let access_flags = reader.u16()?;
        let access_flags = InnerClassAccessFlags::from_bits_truncate(access_flags);

        return Ok(Self {
            inner_class,
            outer_class,
            inner_name,
            access_flags,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation<'a> {
    pub type_descriptor: JavaUTF8<'a>,
    pub element_value_pairs: Vec<ElementValuePair<'a>>,
}

impl<'a> Annotation<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let type_index = reader.u16()?;
        let type_descriptor = ctx.pool.utf8(type_index)?;

        let num_element_value_pairs = reader.u16()? as usize;
        let mut element_value_pairs: Vec<ElementValuePair<'a>> = Vec::with_capacity(num_element_value_pairs);
        for _ in 0..num_element_value_pairs {
            let element_value_pair = ElementValuePair::parse(reader, ctx)?;
            element_value_pairs.push(element_value_pair);
        }

        return Ok(Self {
            type_descriptor,
            element_value_pairs,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParameterAnnotation<'a> {
    pub annotations: Vec<Annotation<'a>>,
}

impl<'a> ParameterAnnotation<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let num_annotations = reader.u16()? as usize;
        let mut annotations: Vec<Annotation<'a>> = Vec::with_capacity(num_annotations);
        for _ in 0..num_annotations {
            let annotation = Annotation::parse(reader, ctx)?;
            annotations.push(annotation);
        }

        return Ok(Self {
            annotations,
        });
    }
}

/// JVMS 4.7.20. An annotation on a *use* of a type, which carries where in the
/// class it applies (`target`) and how far into a nested type (`target_path`).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAnnotation<'a> {
    pub target: TypeAnnotationTarget,
    pub target_path: Vec<TypePathEntry>,
    pub type_descriptor: JavaUTF8<'a>,
    pub element_value_pairs: Vec<ElementValuePair<'a>>,
}

/// The `target_type` byte and its `target_info` (JVMS 4.7.20.1).
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotationTarget {
    /// 0x00 class type parameter, 0x01 method type parameter
    TypeParameter { target_type: u8, type_parameter_index: u8 },
    /// 0x10 extends/implements clause; 0xFFFF means the superclass
    Supertype { supertype_index: u16 },
    /// 0x11 class bound, 0x12 method bound
    TypeParameterBound { target_type: u8, type_parameter_index: u8, bound_index: u8 },
    /// 0x13 field, 0x14 return type, 0x15 receiver
    Empty { target_type: u8 },
    /// 0x16
    FormalParameter { formal_parameter_index: u8 },
    /// 0x17
    Throws { throws_type_index: u16 },
    /// 0x40 local variable, 0x41 resource variable
    LocalVar { target_type: u8, table: Vec<LocalVarTargetEntry> },
    /// 0x42
    Catch { exception_table_index: u16 },
    /// 0x43 instanceof, 0x44 new, 0x45 ::new, 0x46 ::identifier
    Offset { target_type: u8, offset: u16 },
    /// 0x47 cast, 0x48 constructor invocation, 0x49 method invocation,
    /// 0x4A ::new, 0x4B ::identifier
    TypeArgument { target_type: u8, offset: u16, type_argument_index: u8 },
}

impl<'a> TypeAnnotation<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let target_type = reader.u8()?;
        let target = match target_type {
            0x00 | 0x01 => {
                let type_parameter_index = reader.u8()?;
                TypeAnnotationTarget::TypeParameter { target_type, type_parameter_index }
            },
            0x10 => {
                let supertype_index = reader.u16()?;
                TypeAnnotationTarget::Supertype { supertype_index }
            },
            0x11 | 0x12 => {
                let type_parameter_index = reader.u8()?;
                let bound_index = reader.u8()?;
                TypeAnnotationTarget::TypeParameterBound { target_type, type_parameter_index, bound_index }
            },
            0x13 | 0x14 | 0x15 => {
                TypeAnnotationTarget::Empty { target_type }
            },
            0x16 => {
                let formal_parameter_index = reader.u8()?;
                TypeAnnotationTarget::FormalParameter { formal_parameter_index }
            },
            0x17 => {
                let throws_type_index = reader.u16()?;
                TypeAnnotationTarget::Throws { throws_type_index }
            },
            0x40 | 0x41 => {
                let table_length = reader.u16()? as usize;
                let mut table = Vec::with_capacity(table_length);
                for _ in 0..table_length {
                    let start_pc = reader.u16()?;
                    let length = reader.u16()?;
                    let index = reader.u16()?;
                    table.push(LocalVarTargetEntry { start_pc, length, index });
                }
                TypeAnnotationTarget::LocalVar { target_type, table }
            },
            0x42 => {
                let exception_table_index = reader.u16()?;
                TypeAnnotationTarget::Catch { exception_table_index }
            },
            0x43 | 0x44 | 0x45 | 0x46 => {
                let offset = reader.u16()?;
                TypeAnnotationTarget::Offset { target_type, offset }
            },
            0x47 | 0x48 | 0x49 | 0x4A | 0x4B => {
                let offset = reader.u16()?;
                let type_argument_index = reader.u8()?;
                TypeAnnotationTarget::TypeArgument { target_type, offset, type_argument_index }
            },
            _ => return Err(ClassParserError::ClassParseInvalidTypeAnnotationTargetType),
        };

        let path_length = reader.u8()? as usize;
        let mut target_path = Vec::with_capacity(path_length);
        for _ in 0..path_length {
            let type_path_kind = reader.u8()?;
            let type_argument_index = reader.u8()?;
            target_path.push(TypePathEntry { type_path_kind, type_argument_index });
        }

        let type_index = reader.u16()?;
        let type_descriptor = ctx.pool.utf8(type_index)?;

        let num_element_value_pairs = reader.u16()? as usize;
        let mut element_value_pairs = Vec::with_capacity(num_element_value_pairs);
        for _ in 0..num_element_value_pairs {
            let element_value_pair = ElementValuePair::parse(reader, ctx)?;
            element_value_pairs.push(element_value_pair);
        }

        return Ok(Self {
            target,
            target_path,
            type_descriptor,
            element_value_pairs,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalVarTargetEntry {
    pub start_pc: u16,
    pub length: u16,
    pub index: u16,
}

/// One step of a `type_path` (JVMS 4.7.20.2).
#[derive(Debug, Clone, PartialEq)]
pub struct TypePathEntry {
    /// 0 array, 1 nested type, 2 wildcard bound, 3 type argument
    pub type_path_kind: u8,
    pub type_argument_index: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElementValuePair<'a> {
    pub element_name: JavaUTF8<'a>,
    pub value: ElementValue<'a>,
}

impl<'a> ElementValuePair<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let element_name_index = reader.u16()?;
        let element_name = ctx.pool.utf8(element_name_index)?;

        let value = ElementValue::parse(reader, ctx)?;

        return Ok(Self {
            element_name,
            value,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElementValue<'a> {
    Byte(u16),
    Char(u16),
    Double(u16),
    Float(u16),
    Int(u16),
    Long(u16),
    Short(u16),
    Boolean(u16),
    String(u16),
    EnumConstant { type_name: JavaUTF8<'a>, const_name: JavaUTF8<'a> },
    Class(JavaUTF8<'a>),
    Annotation(Annotation<'a>),
    Array(Vec<ElementValue<'a>>),
}

impl<'a> ElementValue<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let tag = reader.u8()?;
        match tag {
            b'B' => {
                let const_value_index = reader.u16()?;
                Ok(ElementValue::Byte(const_value_index))
            },
            b'C' => {
                let const_value_index = reader.u16()?;
                Ok(ElementValue::Char(const_value_index))
            },
            b'D' => {
                let const_value_index = reader.u16()?;
                Ok(ElementValue::Double(const_value_index))
            },
            b'F' => {
                let const_value_index = reader.u16()?;
                Ok(ElementValue::Float(const_value_index))
            },
            b'I' => {
                let const_value_index = reader.u16()?;
                Ok(ElementValue::Int(const_value_index))
            },
            b'J' => {
                let const_value_index = reader.u16()?;
                Ok(ElementValue::Long(const_value_index))
            },
            b'S' => {
                let const_value_index = reader.u16()?;
                Ok(ElementValue::Short(const_value_index))
            },
            b'Z' => {
                let const_value_index = reader.u16()?;
                Ok(ElementValue::Boolean(const_value_index))
            },
            b's' => {
                let const_value_index = reader.u16()?;
                Ok(ElementValue::String(const_value_index))
            },
            b'e' => {
                // Both are Utf8: a field descriptor naming the enum type, and
                // the constant's simple name (JVMS 4.7.16.1).
                let type_name_index = reader.u16()?;
                let type_name = ctx.pool.utf8(type_name_index)?;
                let const_name_index = reader.u16()?;
                let const_name = ctx.pool.utf8(const_name_index)?;
                Ok(ElementValue::EnumConstant { type_name, const_name })
            },
            b'c' => {
                // A return descriptor in a Utf8, not a CONSTANT_Class.
                let class_info_index = reader.u16()?;
                let class_info = ctx.pool.utf8(class_info_index)?;
                Ok(ElementValue::Class(class_info))
            },
            b'@' => {
                let annotation = Annotation::parse(reader, ctx)?;
                Ok(ElementValue::Annotation(annotation))
            },
            b'[' => {
                let num_values = reader.u16()? as usize;
                let mut values: Vec<ElementValue<'a>> = Vec::with_capacity(num_values);
                for _ in 0..num_values {
                    let value = ElementValue::parse(reader, ctx)?;
                    values.push(value);
                }
                Ok(ElementValue::Array(values))
            },
            _ => Err(ClassParserError::ClassParseInvalidElementValueTag),
        }
    }

    pub fn tag(&self) -> u8 {
        match self {
            ElementValue::Byte(_) => b'B',
            ElementValue::Char(_) => b'C',
            ElementValue::Double(_) => b'D',
            ElementValue::Float(_) => b'F',
            ElementValue::Int(_) => b'I',
            ElementValue::Long(_) => b'J',
            ElementValue::Short(_) => b'S',
            ElementValue::Boolean(_) => b'Z',
            ElementValue::String(_) => b's',
            ElementValue::EnumConstant { .. } => b'e',
            ElementValue::Class(_) => b'c',
            ElementValue::Annotation(_) => b'@',
            ElementValue::Array(_) => b'[',
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapMethod {
    /// A CONSTANT_MethodHandle index.
    pub bootstrap_method_ref: u16,
    /// Indices of loadable constants, which may be of several kinds.
    pub bootstrap_arguments: Vec<u16>,
}

impl BootstrapMethod {
    pub fn parse<'a>(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let bootstrap_method_ref = reader.u16()?;

        let num_bootstrap_arguments = reader.u16()? as usize;
        let mut bootstrap_arguments = Vec::with_capacity(num_bootstrap_arguments);
        for _ in 0..num_bootstrap_arguments {
            let argument_index = reader.u16()?;
            bootstrap_arguments.push(argument_index);
        }

        return Ok(Self {
            bootstrap_method_ref,
            bootstrap_arguments,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodParameter<'a> {
    /// `None` when the parameter is unnamed (JVMS 4.7.24).
    pub name: Option<JavaUTF8<'a>>,
    pub access_flags: MethodParameterAccessFlags,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MethodParameterAccessFlags: u16 {
        const FINAL = 0x0010;
        const SYNTHETIC = 0x1000;
        const MANDATED = 0x8000;
    }
}

impl<'a> MethodParameter<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let name_index = reader.u16()?;
        let name = if name_index == 0 {
            None
        } else {
            Some(ctx.pool.utf8(name_index)?)
        };

        let access_flags = reader.u16()?;
        let access_flags = MethodParameterAccessFlags::from_bits_truncate(access_flags);

        return Ok(Self {
            name,
            access_flags,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleAttribute<'a> {
    pub module_name: JavaUTF8<'a>,
    pub module_flags: ModuleFlags,
    pub module_version: Option<JavaUTF8<'a>>,
    pub requires: ModuleRequires<'a>,
    pub exports: ModuleExports<'a>,
    pub opens: ModuleOpens<'a>,
    pub uses: ModuleUses<'a>,
    pub provides: ModuleProvides<'a>,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ModuleFlags: u16 {
        const OPEN = 0x0020;
        const SYNTHETIC = 0x1000;
        const MANDATED = 0x8000;
    }
}

impl<'a> ModuleAttribute<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        if !ctx.at_least(53) {
            return Err(ClassParserError::ClassParseInvalidFeatureUsedForVersion);
        }
        let module_name_index = reader.u16()?;
        let module_name = ctx.pool.module_name(module_name_index)?;

        let module_flags = reader.u16()?;
        let module_flags = ModuleFlags::from_bits_truncate(module_flags);

        let module_version_index = reader.u16()?;
        let module_version = if module_version_index == 0 {
            None
        } else {
            Some(ctx.pool.utf8(module_version_index)?)
        };

        let requires = ModuleRequires::parse(reader, ctx)?;
        let exports = ModuleExports::parse(reader, ctx)?;
        let opens = ModuleOpens::parse(reader, ctx)?;
        let uses = ModuleUses::parse(reader, ctx)?;
        let provides = ModuleProvides::parse(reader, ctx)?;

        return Ok(Self {
            module_name,
            module_flags,
            module_version,
            requires,
            exports,
            opens,
            uses,
            provides,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleRequiresEntry<'a> {
    pub requires: JavaUTF8<'a>,
    pub requires_flags: ModuleRequiresFlags,
    pub requires_version: Option<JavaUTF8<'a>>,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ModuleRequiresFlags: u16 {
        const TRANSITIVE = 0x0020;
        const STATIC_PHASE = 0x0040;
        const SYNTHETIC = 0x1000;
        const MANDATED = 0x8000;
    }
}

impl<'a> ModuleRequiresEntry<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let requires_index = reader.u16()?;
        let requires = ctx.pool.module_name(requires_index)?;

        let requires_flags = reader.u16()?;
        let requires_flags = ModuleRequiresFlags::from_bits_truncate(requires_flags);

        let requires_version_index = reader.u16()?;
        let requires_version = if requires_version_index == 0 {
            None
        } else {
            Some(ctx.pool.utf8(requires_version_index)?)
        };

        return Ok(Self {
            requires,
            requires_flags,
            requires_version,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleExportsEntry<'a> {
    pub exports: JavaUTF8<'a>,
    pub exports_flags: ModuleExportsFlags,
    pub exports_to: Vec<JavaUTF8<'a>>,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ModuleExportsFlags: u16 {
        const SYNTHETIC = 0x1000;
        const MANDATED = 0x8000;
    }
}

impl<'a> ModuleExportsEntry<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let exports_index = reader.u16()?;
        let exports = ctx.pool.package_name(exports_index)?;

        let exports_flags = reader.u16()?;
        let exports_flags = ModuleExportsFlags::from_bits_truncate(exports_flags);

        let exports_to_count = reader.u16()? as usize;
        let mut exports_to = Vec::with_capacity(exports_to_count);
        for _ in 0..exports_to_count {
            let exports_to_index = reader.u16()?;
            if exports_to_index == 0 {
                return Err(ClassParserError::ClassParseInvalidConstantPoolIndex);
            }
            let exports_to_class = ctx.pool.module_name(exports_to_index)?;
            exports_to.push(exports_to_class);
        }

        return Ok(Self {
            exports,
            exports_flags,
            exports_to,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleOpensEntry<'a> {
    pub opens: JavaUTF8<'a>,
    pub opens_flags: ModuleOpensFlags,
    pub opens_to: Vec<JavaUTF8<'a>>,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ModuleOpensFlags: u16 {
        const SYNTHETIC = 0x1000;
        const MANDATED = 0x8000;
    }
}

impl<'a> ModuleOpensEntry<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let opens_index = reader.u16()?;
        let opens = ctx.pool.package_name(opens_index)?;

        let opens_flags = reader.u16()?;
        let opens_flags = ModuleOpensFlags::from_bits_truncate(opens_flags);

        let opens_to_count = reader.u16()? as usize;
        let mut opens_to = Vec::with_capacity(opens_to_count);
        for _ in 0..opens_to_count {
            let opens_to_index = reader.u16()?;
            let opens_to_class = ctx.pool.module_name(opens_to_index)?;
            opens_to.push(opens_to_class);
        }

        return Ok(Self {
            opens,
            opens_flags,
            opens_to,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleProvidesEntry<'a> {
    pub provides: JavaUTF8<'a>,
    pub provides_with: Vec<JavaUTF8<'a>>,
}

impl<'a> ModuleProvidesEntry<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let provides_index = reader.u16()?;
        let provides = ctx.pool.class_name(provides_index)?;

        let provides_with_count = reader.u16()? as usize;
        let mut provides_with = Vec::with_capacity(provides_with_count);
        for _ in 0..provides_with_count {
            let provides_with_index = reader.u16()?;
            let provides_with_class = ctx.pool.class_name(provides_with_index)?;
            provides_with.push(provides_with_class);
        }

        return Ok(Self {
            provides,
            provides_with,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordAttribute<'a> {
    pub record_components: Vec<RecordComponentInfo<'a>>,
}

impl<'a> RecordAttribute<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let record_components_count = reader.u16()? as usize;
        let mut record_components = Vec::with_capacity(record_components_count);
        for _ in 0..record_components_count {
            let record_component = RecordComponentInfo::parse(reader, ctx)?;
            record_components.push(record_component);
        }

        return Ok(Self {
            record_components,
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordComponentInfo<'a> {
    pub name: JavaUTF8<'a>,
    pub descriptor: JavaUTF8<'a>,
    pub attributes: Attributes<'a>,
}

impl<'a> RecordComponentInfo<'a> {
    pub fn parse(reader: &mut Reader<'a>, ctx: &ClassParseCtx<'a>) -> Result<Self, ClassParserError> {
        let name_index = reader.u16()?;
        let name = ctx.pool.utf8(name_index)?;

        let descriptor_index = reader.u16()?;
        let descriptor = ctx.pool.utf8(descriptor_index)?;

        let attributes = Attributes::parse(reader, ctx, AttributeLocation::RecordComponent)?;

        return Ok(Self {
            name,
            descriptor,
            attributes,
        });
    }
}
