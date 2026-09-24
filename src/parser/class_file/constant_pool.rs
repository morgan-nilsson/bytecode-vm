use crate::parser::reader::Reader;
use crate::parser::class_file::ClassParseCtx;
use crate::java_utf::{ JavaUTF8, JavaUTF8Error };

use super::error::ClassParserError;
use super::descriptor::valid_class_entry_name;

/// The constant pool, indexed from 1 (JVMS 4.4). Dereferences to a slice, so
/// `len()` and iteration work; `get` applies the 1-based indexing and refuses
/// the unusable slot after a Long or Double.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConstantPool<'a> {
    pub entries: Vec<ConstantPoolEntry<'a>>,
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

    /// The CONSTANT_NameAndType at `index`, checked for kind.
    pub fn name_and_type(&self, index: u16) -> Result<(u16, u16), ClassParserError> {
        match self.require(index)? {
            ConstantPoolEntry::NameAndType { name_index, descriptor_index } => {
                Ok((*name_index, *descriptor_index))
            }
            _ => Err(ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry),
        }
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
                // The referent is checked by validate_references, once the pool
                // is whole — it may appear after this entry.
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