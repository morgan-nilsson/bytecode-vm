use bitflags::bitflags;

use crate::java_utf::JavaUTF8;
use crate::parser::reader::Reader;

use super::attribute::{AttributeLocation, Attributes};
use super::error::ClassParserError;
use super::ClassParseCtx;
use super::constant_pool::ConstantPool;

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

    pub fn verify(&self, ctx: &ClassParseCtx) -> Result<(), ClassParserError> {
        return Ok(());
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

    pub fn verify(&self, ctx: &ClassParseCtx) -> Result<(), ClassParserError> {
        return Ok(());
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

    pub fn verify(&self, ctx: &ClassParseCtx) -> Result<(), ClassParserError> {
        return Ok(());
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
