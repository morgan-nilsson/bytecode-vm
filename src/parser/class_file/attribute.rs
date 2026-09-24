use bitflags::bitflags;

use crate::parser::class_file::error::ClassParserError;
use crate::parser::class_file::{ClassFileAccessFlags, ClassParseCtx};
use crate::parser::reader::Reader;
use crate::java_utf::JavaUTF8;

use super::stack_map::StackMapFrame;
use super::annotation::{Annotation, ParameterAnnotation, TypeAnnotation, ElementValue};
use super::module::ModuleAttribute;
use super::constant_pool::ConstantPool;

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

    pub fn verify(&self, ctx: &ClassParseCtx) -> Result<(), ClassParserError> {
        return Ok(());
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
