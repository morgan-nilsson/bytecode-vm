use crate::parser::class_file::error::ClassParserError;
use crate::parser::class_file::ClassParseCtx;
use crate::parser::reader::Reader;

use super::constant_pool::ConstantPoolEntry;

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