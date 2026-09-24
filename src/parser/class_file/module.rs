use bitflags::bitflags;

use crate::parser::class_file::error::ClassParserError;
use crate::parser::class_file::ClassParseCtx;
use crate::parser::reader::Reader;
use crate::java_utf::JavaUTF8;

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