use crate::parser::reader::Reader;
use crate::parser::class_file::ClassParseCtx;
use crate::java_utf::{ JavaUTF8, JavaUTF8Error };
use crate::parser::class_file::ClassFileAccessFlags;

use super::error::ClassParserError;
use super::descriptor::{valid_unqualified_name, valid_class_entry_name, valid_field_descriptor, valid_method_descriptor};

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
        Ok(pool)
    }

    /// Checks every entry's references. This cannot happen while reading,
    /// because an entry may name one that appears later in the table
    pub fn verify(&self, ctx: &ClassParseCtx) -> Result<(), ClassParserError> {
        for entry in &self.entries {
            match entry {
                ConstantPoolEntry::ClassIndex(n) => {
                    let name = self.utf8(*n)?;
                    if !valid_class_entry_name(name) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }
                }

                ConstantPoolEntry::StringIndex(n) => {
                    self.utf8(*n)?;
                }

                // Each of these names a Utf8 as its only reference.
                ConstantPoolEntry::ModuleIndex(n)
                | ConstantPoolEntry::PackageIndex(n) => {
                    if !ctx.at_least(53) {
                        return Err(ClassParserError::ClassParseInvalidFeatureUsedForVersion);
                    }
                    self.utf8(*n)?;
                    if !ctx.access_flags.contains(ClassFileAccessFlags::MODULE) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }
                }

                ConstantPoolEntry::MethodTypeIndex(n) => {
                    if !ctx.at_least(51) {
                        return Err(ClassParserError::ClassParseInvalidFeatureUsedForVersion);
                    }
                    let index = self.utf8(*n)?;
                    if !valid_method_descriptor(index) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }
                }

                ConstantPoolEntry::NameAndType { name_index, descriptor_index } => {
                    let name = self.utf8(*name_index)?;
                    let desc = self.utf8(*descriptor_index)?;
                    if !valid_unqualified_name(name) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }
                    if !valid_field_descriptor(desc) && !valid_method_descriptor(desc) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }
                }

                ConstantPoolEntry::FieldRef { class_index, name_and_type_index } => {
                    self.class_name(*class_index)?;
                    let nat = self.name_and_type(*name_and_type_index)?;   // → (name_index, descriptor_index)
                    let name = self.utf8(nat.0)?;
                    let desc = self.utf8(nat.1)?;
                    if !valid_unqualified_name(name) || !valid_field_descriptor(desc) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }

                }

                ConstantPoolEntry::MethodRef { class_index, name_and_type_index }
                | ConstantPoolEntry::InterfaceMethodRef { class_index, name_and_type_index } => {
                    let is_interface = matches!(entry, ConstantPoolEntry::InterfaceMethodRef { .. });
                    self.class_name(*class_index)?;
                    let nat = self.name_and_type(*name_and_type_index)?;
                    let name = self.utf8(nat.0)?;
                    let desc = self.utf8(nat.1)?;

                    if !valid_method_descriptor(desc) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }

                    let n = name.as_bytes();
                    if n.first() == Some(&b'<') {
                        // Only <init>, only on Methodref, and it must return void.
                        if is_interface || n != b"<init>" || !desc.as_bytes().ends_with(b")V") {
                            return Err(ClassParserError::ClassParseFormatError);
                        }
                    } else if !valid_unqualified_name(name) || n.contains(&b'>') {
                        return Err(ClassParserError::ClassParseFormatError);
                    }
                }

                // JVMS 4.4.10 constrains the *descriptor* of the NameAndType:
                // a field descriptor for Dynamic, a method descriptor for
                // InvokeDynamic. The name is an ordinary member name.
                ConstantPoolEntry::Dynamic { name_and_type_index, .. } => {
                    let (name_index, descriptor_index) =
                        self.name_and_type(*name_and_type_index)?;
                    if !valid_unqualified_name(self.utf8(name_index)?)
                        || !valid_field_descriptor(self.utf8(descriptor_index)?) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }

                    // There must be a bootstrap 
                    if !ctx.attributes.as_ref().unwrap().iter().any(|attr| matches!(attr, super::attribute::AttributeInfo::BootstrapMethods { .. })) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }

                }

                ConstantPoolEntry::InvokeDynamic { bootstrap_index, name_and_type_index} => {
                    let (name_index, descriptor_index) =
                        self.name_and_type(*name_and_type_index)?;
                    if !valid_unqualified_name(self.utf8(name_index)?)
                        || !valid_method_descriptor(self.utf8(descriptor_index)?) {
                        return Err(ClassParserError::ClassParseFormatError);
                    }

                    // bootstrap_index must be in range of the BootstrapMethods attribute
                    let bootstrap_methods = ctx.attributes
                            .as_ref()
                            .ok_or(ClassParserError::ClassParseFormatError)?
                            .iter()
                            .find_map(|attr| {
                        if let super::attribute::AttributeInfo::BootstrapMethods { value } = attr {
                            Some(value)
                        } else {
                            None
                        }
                    }).ok_or(ClassParserError::ClassParseFormatError)?;

                    if usize::from(*bootstrap_index) >= bootstrap_methods.len() {
                        return Err(ClassParserError::ClassParseFormatError);
                    }
                }

                // Which kind the reference must be depends on ref_kind, so only
                // its existence is settled here (JVMS 4.4.8 has the rest).
                ConstantPoolEntry::MethodHandle { ref_kind, ref_index } => {
                    let target = self.member_ref(*ref_index)?;
                    if *ref_kind == MethodHandleKind::InvokeStatic || *ref_kind == MethodHandleKind::InvokeSpecial {
                        if matches!(target, ConstantPoolEntry::InterfaceMethodRef { .. }) {
                            if !ctx.at_least(52) {
                                return Err(ClassParserError::ClassParseInvalidFeatureUsedForVersion);
                            }
                        }
                    }

                    let ok_tag = match (ref_kind, target) {
                        (MethodHandleKind::GetField, ConstantPoolEntry::FieldRef { .. }) => true,
                        (MethodHandleKind::GetStatic, ConstantPoolEntry::FieldRef { .. }) => true,
                        (MethodHandleKind::PutField, ConstantPoolEntry::FieldRef { .. }) => true,
                        (MethodHandleKind::PutStatic, ConstantPoolEntry::FieldRef { .. }) => true,
                        (MethodHandleKind::InvokeVirtual, ConstantPoolEntry::MethodRef { .. }) => true,
                        (MethodHandleKind::InvokeStatic, ConstantPoolEntry::MethodRef { .. }) => true,
                        (MethodHandleKind::InvokeSpecial, ConstantPoolEntry::MethodRef { .. }) => true,
                        (MethodHandleKind::NewInvokeSpecial, ConstantPoolEntry::MethodRef { .. }) => true,
                        (MethodHandleKind::InvokeInterface, ConstantPoolEntry::InterfaceMethodRef { .. }) => true,
                        (MethodHandleKind::InvokeStatic, ConstantPoolEntry::InterfaceMethodRef { .. }) => true,
                        (MethodHandleKind::InvokeSpecial, ConstantPoolEntry::InterfaceMethodRef { .. }) => true,
                        _ => false
                    };
                    if !ok_tag {
                        return Err(ClassParserError::ClassParseFormatError);
                    }

                    let bad_name = match ref_kind {
                        MethodHandleKind::InvokeVirtual
                        | MethodHandleKind::InvokeStatic
                        | MethodHandleKind::InvokeSpecial
                        | MethodHandleKind::InvokeInterface => {
                            let n = self.member_ref_name(*ref_index)?;
                            n == "<init>" || n == "<clinit>"
                        }
                        MethodHandleKind::NewInvokeSpecial => {
                            self.member_ref_name(*ref_index)? != "<init>"
                        }
                        _ => false,
                    };
                    {
                        if bad_name {
                            return Err(ClassParserError::ClassParseFormatError);
                        }

                    }
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

    /// The member reference at `index` — Fieldref, Methodref or
    /// InterfaceMethodref. Which of the three a caller requires is its own
    /// rule; naming something that is no member reference at all is a bad
    /// reference rather than a format error.
    pub fn member_ref(&self, index: u16) -> Result<&ConstantPoolEntry<'a>, ClassParserError> {
        match self.require(index)? {
            entry @ (ConstantPoolEntry::FieldRef { .. }
            | ConstantPoolEntry::MethodRef { .. }
            | ConstantPoolEntry::InterfaceMethodRef { .. }) => Ok(entry),
            _ => Err(ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry),
        }
    }

    /// The name the member reference at `index` points at.
    pub fn member_ref_name(&self, index: u16) -> Result<JavaUTF8<'a>, ClassParserError> {
        let nat_index = match self.member_ref(index)? {
            ConstantPoolEntry::FieldRef { name_and_type_index, .. }
            | ConstantPoolEntry::MethodRef { name_and_type_index, .. }
            | ConstantPoolEntry::InterfaceMethodRef { name_and_type_index, .. } => {
                *name_and_type_index
            }
            _ => unreachable!("member_ref only returns the three member reference kinds"),
        };
        let (name_index, _descriptor_index) = self.name_and_type(nat_index)?;
        self.utf8(name_index)
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