use thiserror::Error;
use crate::java_utf::JavaUTF8Error;
use crate::parser::reader::ParseError;

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
