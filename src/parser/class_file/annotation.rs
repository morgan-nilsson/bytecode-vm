use crate::parser::class_file::error::ClassParserError;
use crate::parser::class_file::ClassParseCtx;
use crate::parser::reader::Reader;
use crate::java_utf::JavaUTF8;

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