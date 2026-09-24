use bitflags::{bitflags};

// The submodules are an organisational split, not a public hierarchy: every
// type is re-exported here, so callers and tests use one flat path,
// `parser::class_file::AttributeInfo`, wherever the item happens to live.
pub mod error;
pub use error::*;

pub mod constant_pool;
pub use constant_pool::*;

pub mod stack_map;
pub use stack_map::*;

pub mod annotation;
pub use annotation::*;

pub mod module;
pub use module::*;

pub mod attribute;
pub use attribute::*;

pub mod member;
pub use member::*;

// Name and descriptor checks are internal to parsing, so they are not re-exported.
pub mod descriptor;
use descriptor::valid_class_name;

use crate::java_utf::JavaUTF8;
use crate::parser::reader::{Reader};


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

impl ClassFileAccessFlags {
    pub fn verify(&self, ctx: &ClassParseCtx) -> Result<(), ClassParserError> {
        
        if ctx.access_flags.contains(ClassFileAccessFlags::ANNOTATION) && !ctx.access_flags.contains(ClassFileAccessFlags::INTERFACE) {
            return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
        }

        if ctx.access_flags.contains(ClassFileAccessFlags::FINAL) && ctx.access_flags.contains(ClassFileAccessFlags::ABSTRACT) {
            return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
        }

        if ctx.access_flags.contains(ClassFileAccessFlags::INTERFACE) && !ctx.access_flags.contains(ClassFileAccessFlags::ABSTRACT) {
            return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
        }

        if ctx.access_flags.contains(ClassFileAccessFlags::MODULE) {
            if ctx.access_flags.contains(ClassFileAccessFlags::FINAL)
                || ctx.access_flags.contains(ClassFileAccessFlags::SUPER)
                || ctx.access_flags.contains(ClassFileAccessFlags::INTERFACE)
                || ctx.access_flags.contains(ClassFileAccessFlags::ABSTRACT)
                || ctx.access_flags.contains(ClassFileAccessFlags::ANNOTATION)
                || ctx.access_flags.contains(ClassFileAccessFlags::ENUM)
            {
                return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
            }
        }

        if ctx.access_flags.contains(ClassFileAccessFlags::INTERFACE) {
            if ctx.access_flags.contains(ClassFileAccessFlags::ENUM)
                || ctx.access_flags.contains(ClassFileAccessFlags::FINAL)
                || ctx.access_flags.contains(ClassFileAccessFlags::SUPER)
            {
                return Err(ClassParserError::ClassParseInvalidAccessFlagsCombination);
            }
        }

        return Ok(());
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
    pub interfaces: Option<Interfaces<'a>>,
    pub fields: Option<Fields<'a>>,
    pub methods: Option<Methods<'a>>,
    pub attributes: Option<Attributes<'a>>,
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
            interfaces: None,
            fields: None,
            methods: None,
            attributes: None,
        }
    }

    fn with_interfaces(mut self, interfaces: Interfaces<'a>) -> Self {
        self.interfaces = Some(interfaces);
        self
    }

    fn with_fields(mut self, fields: Fields<'a>) -> Self {
        self.fields = Some(fields);
        self
    }

    fn with_methods(mut self, methods: Methods<'a>) -> Self {
        self.methods = Some(methods);
        self
    }

    fn with_attributes(mut self, attributes: Attributes<'a>) -> Self {
        self.attributes = Some(attributes);
        self
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
        let mut parser_ctx = ClassParseCtx::new(
            major_version,
            minor_version,
            constant_pool.clone(),
            access_flags,
            this_class_name,
            super_class,
        );
        parser_ctx = parser_ctx.with_interfaces(interfaces.clone());

        let fields = Fields::parse(reader, &parser_ctx)?;
        parser_ctx = parser_ctx.with_fields(fields.clone());

        let methods = Methods::parse(reader, &parser_ctx)?;
        parser_ctx = parser_ctx.with_methods(methods.clone());

        let attributes = Attributes::parse(reader, &parser_ctx, AttributeLocation::ClassFile)?;
        parser_ctx = parser_ctx.with_attributes(attributes.clone());

        // if reader still has bytes left, return an error
        if !reader.is_empty() {
            return Err(ClassParserError::ClassParseTrailingBytes);
        }

        // Nothing to do for major and minor version
        constant_pool.verify(&parser_ctx)?;
        access_flags.verify(&parser_ctx)?;
        // Nothing to do for this_class and super_class
        interfaces.verify(&parser_ctx)?;
        fields.verify(&parser_ctx)?;
        methods.verify(&parser_ctx)?;
        attributes.verify(&parser_ctx)?;

        return Ok(Self {
            minor_version,
            major_version,
            constant_pool,
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
