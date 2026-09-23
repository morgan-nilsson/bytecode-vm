//! The StackMapTable attribute (JVMS 4.7.4): every stack map frame type, every
//! verification type, and the attribute that carries them.
//! Ported from `c_backup/tests/test_classfile_stack_map.c`.
//!
//! The C suite fed bytes straight to `stack_map_frame_read`. Here the only
//! entry point is `ClassFile::parse`, so every frame under test rides inside
//! the StackMapTable of a generated method's Code attribute.


use crate::common::*;

use bytecode_vm::parser::class_file::{
    AttributeInfo, ClassParserError, StackMapFrame, VerificationTypeInfo,
};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// Constant pool indices the generated frames can point at.
struct PoolRefs {
    /// A CONSTANT_Class: the only entry kind an ObjectVariableInfo may name.
    object_class: u16,
    /// The "StackMapTable" CONSTANT_Utf8, doubling as an in-range index of the
    /// wrong kind for ObjectVariableInfo.
    utf8: u16,
    /// The unusable slot after a CONSTANT_Long (JVMS 4.4.5).
    long_second_slot: u16,
    /// One past the last valid index.
    past_end: u16,
}

/// `public static void m()` whose Code carries a StackMapTable with the body
/// `build` writes. The body is everything after `attribute_length`, so tests
/// that care about `number_of_entries` disagreeing with the frames can say so.
fn class_with_stack_map_body(build: impl FnOnce(&mut Bytes, &PoolRefs)) -> Vec<u8> {
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let object_class = cb.pool.class("java/lang/String");
    let utf8 = cb.pool.utf8("StackMapTable");
    let long = cb.pool.long(1);
    let code_name = cb.pool.utf8("Code");
    cb.add_method(acc::PUBLIC | acc::STATIC, "m", "()V", 1);

    // Taken after every pool entry exists, so `past_end` really is out of range.
    let refs = PoolRefs {
        object_class,
        utf8,
        long_second_slot: long + 1,
        past_end: cb.pool.next,
    };
    let mut body = Bytes::new();
    build(&mut body, &refs);

    let code = cb.methods.attr_begin(code_name);
    cb.methods.u2(0); // max_stack
    cb.methods.u2(0); // max_locals
    cb.methods.u4(1);
    cb.methods.u1(op::RETURN);
    cb.methods.u2(0); // exception_table_length
    cb.methods.u2(1); // attributes_count
    let smt = cb.methods.attr_begin(utf8);
    cb.methods.cat(&body);
    cb.methods.attr_end(smt);
    cb.methods.attr_end(code);
    cb.to_bytes()
}

/// The same class, with `number_of_entries` written for you.
fn class_with_frames(count: u16, build: impl FnOnce(&mut Bytes, &PoolRefs)) -> Vec<u8> {
    class_with_stack_map_body(|b, refs| {
        b.u2(count);
        build(b, refs);
    })
}

/// Parses a generated class and hands over its StackMapTable entries.
fn with_entries<T>(bytes: &[u8], f: impl FnOnce(&[StackMapFrame]) -> T) -> T {
    assert_parses(bytes, |cf| {
        let method = cf.methods.first().expect("the generated class has one method");
        let code = method
            .attributes
            .iter()
            .find_map(|a| match a {
                AttributeInfo::Code { attributes, .. } => Some(attributes),
                _ => None,
            })
            .expect("the method has a Code attribute");
        let entries = code
            .iter()
            .find_map(|a| match a {
                AttributeInfo::StackMapTable { entries } => Some(entries),
                _ => None,
            })
            .expect("the Code attribute has a StackMapTable");
        f(entries)
    })
}

/// Builds a table holding exactly one frame and hands that frame over.
fn with_frame<T>(
    build: impl FnOnce(&mut Bytes, &PoolRefs),
    f: impl FnOnce(&StackMapFrame) -> T,
) -> T {
    let bytes = class_with_frames(1, build);
    with_entries(&bytes, |entries| {
        assert_eq!(entries.len(), 1, "expected exactly one frame");
        f(&entries[0])
    })
}

/// A frame's variant, for assertion messages. The enum has no `Debug`.
fn frame_kind(frame: &StackMapFrame) -> &'static str {
    match frame {
        StackMapFrame::SameFrame { .. } => "same_frame",
        StackMapFrame::SameLocals1StackItemFrame { .. } => "same_locals_1_stack_item_frame",
        StackMapFrame::SameLocals1StackItemFrameExtended { .. } => {
            "same_locals_1_stack_item_frame_extended"
        }
        StackMapFrame::ChopFrame { .. } => "chop_frame",
        StackMapFrame::SameFrameExtended { .. } => "same_frame_extended",
        StackMapFrame::AppendFrame { .. } => "append_frame",
        StackMapFrame::FullFrame { .. } => "full_frame",
    }
}

/// A verification type as text, payload included, so a whole list can be
/// compared in one assertion that still reads as JVMS names.
fn item_desc(info: &VerificationTypeInfo) -> String {
    match info {
        VerificationTypeInfo::TopVariableInfo => "Top".to_string(),
        VerificationTypeInfo::IntegerVariableInfo => "Integer".to_string(),
        VerificationTypeInfo::FloatVariableInfo => "Float".to_string(),
        VerificationTypeInfo::LongVariableInfo => "Long".to_string(),
        VerificationTypeInfo::DoubleVariableInfo => "Double".to_string(),
        VerificationTypeInfo::NullVariableInfo => "Null".to_string(),
        VerificationTypeInfo::UninitializedThisVariableInfo => "UninitializedThis".to_string(),
        VerificationTypeInfo::ObjectVariableInfo { cpool_index } => format!("Object(#{cpool_index})"),
        VerificationTypeInfo::UninitializedVariableInfo { offset } => {
            format!("Uninitialized(@{offset})")
        }
    }
}

/// A list of verification types as `"A, B, C"`.
fn items_desc(infos: &[VerificationTypeInfo]) -> String {
    infos.iter().map(item_desc).collect::<Vec<_>>().join(", ")
}

/// The single stack item of a `same_locals_1_stack_item_frame`, whichever of
/// the two forms it took, plus the frame's raw `frame_type`.
#[track_caller]
fn one_stack_item(frame: &StackMapFrame) -> (u8, String) {
    match frame {
        StackMapFrame::SameLocals1StackItemFrame { frame_type, stack } => {
            (*frame_type, item_desc(stack))
        }
        StackMapFrame::SameLocals1StackItemFrameExtended { frame_type, stack, .. } => {
            (*frame_type, item_desc(stack))
        }
        other => panic!("expected a one-stack-item frame, got {}", frame_kind(other)),
    }
}

/// A one-byte verification type in a `same_locals_1_stack_item_frame`, the
/// shortest frame that carries a verification type at all.
fn stack_item_frame(b: &mut Bytes, tag: u8) {
    b.u1(64);
    b.u1(tag);
}

// ---------------------------------------------------------------------------
// verification_type_info
// ---------------------------------------------------------------------------

#[test]
fn verification_type_single_byte_tags() {
    // Tags 0..=6 are the whole type, with no operand following.
    for (tag, expected) in [
        (item::TOP, "Top"),
        (item::INTEGER, "Integer"),
        (item::FLOAT, "Float"),
        (item::DOUBLE, "Double"),
        (item::LONG, "Long"),
        (item::NULL, "Null"),
        (item::UNINITIALIZED_THIS, "UninitializedThis"),
    ] {
        with_frame(
            |b, _| stack_item_frame(b, tag),
            |frame| {
                let (_, desc) = one_stack_item(frame);
                assert_eq!(desc, expected, "tag {tag} parsed wrong");
            },
        );
    }
}

#[test]
fn verification_type_object() {
    with_frame(
        |b, refs| {
            b.u1(64);
            b.u1(item::OBJECT);
            b.u2(refs.object_class);
        },
        |frame| {
            let (_, desc) = one_stack_item(frame);
            // The index is kept raw; the class it names is resolved later.
            assert!(desc.starts_with("Object(#"), "got {desc}");
        },
    );
}

#[test]
fn verification_type_uninitialized() {
    // The operand is a bytecode offset, not a pool index, so any value goes.
    with_frame(
        |b, _| {
            b.u1(64);
            b.u1(item::UNINITIALIZED);
            b.u2(0x0123);
        },
        |frame| assert_eq!(one_stack_item(frame).1, "Uninitialized(@291)"),
    );
}

#[test]
fn verification_type_invalid_tags_rejected() {
    // 9 and up are unassigned; a parser that fell through to a default would
    // silently accept them.
    for tag in [9u8, 10, 0x7f, 0xff] {
        let bytes = class_with_frames(1, |b, _| {
            stack_item_frame(b, tag);
            b.u2(0);
        });
        assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidVerificationTypeTag);
    }
}

#[test]
fn verification_type_object_index_zero_rejected() {
    // Pool index 0 is never a valid entry (JVMS 4.4).
    let bytes = class_with_frames(1, |b, _| {
        b.u1(64);
        b.u1(item::OBJECT);
        b.u2(0);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn verification_type_object_index_out_of_range_rejected() {
    let bytes = class_with_frames(1, |b, refs| {
        b.u1(64);
        b.u1(item::OBJECT);
        b.u2(refs.past_end);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn verification_type_object_not_class_rejected() {
    // An in-range index of the wrong kind: the entry has to be CONSTANT_Class.
    let bytes = class_with_frames(1, |b, refs| {
        b.u1(64);
        b.u1(item::OBJECT);
        b.u2(refs.utf8);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseReferenceToInvalidConstantPoolEntry);
}

#[test]
fn verification_type_object_unusable_long_slot_rejected() {
    // Not in the C suite: the slot after a CONSTANT_Long is in range but holds
    // no entry, which a bounds-only check would wave through.
    let bytes = class_with_frames(1, |b, refs| {
        b.u1(64);
        b.u1(item::OBJECT);
        b.u2(refs.long_second_slot);
    });
    // In range, but it names no entry at all, so it is an index fault rather
    // than a wrong-kind reference.
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidConstantPoolIndex);
}

#[test]
fn verification_type_truncated_rejected() {
    // The frame runs past the end of the attribute, not past the end of the
    // file, so the length is what disagrees.

    // A tag byte that never arrives.
    assert_rejected_with!(
        &class_with_frames(1, |b, _| {
            b.u1(64);
        }),
        ClassParserError::ClassParseInvalidAttributeLength
    );

    // An Object tag with only half of its index.
    assert_rejected_with!(
        &class_with_frames(1, |b, _| {
            b.u1(64);
            b.u1(item::OBJECT);
            b.u1(0);
        }),
        ClassParserError::ClassParseInvalidAttributeLength
    );
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

#[test]
fn same_frame_range() {
    // The frame type byte is itself the offset delta for 0..=63.
    for t in [0u8, 1, 32, 63] {
        with_frame(
            |b, _| {
                b.u1(t);
            },
            |frame| match frame {
                StackMapFrame::SameFrame { frame_type } => assert_eq!(*frame_type, t),
                other => panic!("frame_type {t} parsed as {}", frame_kind(other)),
            },
        );
    }
}

#[test]
fn same_locals_1_stack_item_frame_range() {
    // Both ends of 64..=127, so an off-by-one at either bound shows up.
    with_frame(
        |b, _| stack_item_frame(b, item::INTEGER),
        |frame| assert_eq!(one_stack_item(frame), (64, "Integer".to_string())),
    );

    with_frame(
        |b, refs| {
            b.u1(127);
            b.u1(item::OBJECT);
            b.u2(refs.object_class);
        },
        |frame| {
            let (frame_type, desc) = one_stack_item(frame);
            assert_eq!(frame_type, 127);
            assert!(desc.starts_with("Object(#"), "got {desc}");
        },
    );
}

#[test]
fn reserved_frame_types_rejected() {
    // 128..=246 are reserved for future use (JVMS 4.7.4). The eight filler
    // bytes make sure rejection is about the type, not about running out.
    for t in 128u8..=246 {
        let bytes = class_with_frames(1, |b, _| {
            b.u1(t);
            b.fill(0, 8);
        });
        assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidStackMapFrameType);
    }
}

#[test]
fn same_locals_1_stack_item_frame_extended() {
    // 247 is the only type between the short form and the chop range.
    with_frame(
        |b, _| {
            b.u1(247);
            b.u2(0x1234);
            b.u1(item::LONG);
        },
        |frame| match frame {
            StackMapFrame::SameLocals1StackItemFrameExtended {
                frame_type,
                offset_delta,
                stack,
            } => {
                assert_eq!(*frame_type, 247);
                assert_eq!(*offset_delta, 0x1234);
                assert_eq!(item_desc(stack), "Long");
            }
            other => panic!("expected the extended form, got {}", frame_kind(other)),
        },
    );
}

#[test]
fn chop_frame_range() {
    // All three of 248..=250; the count chopped is 251 - frame_type, which the
    // parser keeps implicitly by keeping the raw type.
    for t in 248u8..=250 {
        let delta = 300 + u16::from(t);
        with_frame(
            |b, _| {
                b.u1(t);
                b.u2(delta);
            },
            |frame| match frame {
                StackMapFrame::ChopFrame { frame_type, offset_delta } => {
                    assert_eq!(*frame_type, t);
                    assert_eq!(*offset_delta, delta);
                }
                other => panic!("frame_type {t} parsed as {}", frame_kind(other)),
            },
        );
    }
}

#[test]
fn same_frame_extended() {
    // 0xFFFF is the largest offset_delta the format can express.
    with_frame(
        |b, _| {
            b.u1(251);
            b.u2(0xFFFF);
        },
        |frame| match frame {
            StackMapFrame::SameFrameExtended { frame_type, offset_delta } => {
                assert_eq!(*frame_type, 251);
                assert_eq!(*offset_delta, 0xFFFF);
            }
            other => panic!("expected same_frame_extended, got {}", frame_kind(other)),
        },
    );
}

#[test]
fn append_frame_with_one_local() {
    with_frame(
        |b, _| {
            b.u1(252);
            b.u2(5);
            b.u1(item::FLOAT);
        },
        |frame| match frame {
            StackMapFrame::AppendFrame { frame_type, offset_delta, locals } => {
                assert_eq!(*frame_type, 252);
                assert_eq!(*offset_delta, 5);
                assert_eq!(items_desc(locals), "Float");
            }
            other => panic!("expected append_frame, got {}", frame_kind(other)),
        },
    );
}

#[test]
fn append_frame_with_three_locals() {
    // 254 means three locals, and a two-byte Object in the middle checks that
    // the locals are read one after another rather than at fixed offsets.
    let bytes = class_with_frames(1, |b, refs| {
        b.u1(254);
        b.u2(17);
        b.u1(item::INTEGER);
        b.u1(item::OBJECT);
        b.u2(refs.object_class);
        b.u1(item::DOUBLE);
    });
    with_entries(&bytes, |entries| match &entries[0] {
        StackMapFrame::AppendFrame { locals, .. } => {
            assert_eq!(locals.len(), 3);
            assert_eq!(items_desc(&locals[..1]), "Integer");
            assert!(item_desc(&locals[1]).starts_with("Object(#"));
            assert_eq!(items_desc(&locals[2..]), "Double");
        }
        other => panic!("expected append_frame, got {}", frame_kind(other)),
    });
}

#[test]
fn append_frame_253_has_two_locals() {
    // The middle of the append range: k is frame_type - 251, so 253 is two.
    with_frame(
        |b, _| {
            b.u1(253);
            b.u2(0);
            b.u1(item::NULL);
            b.u1(item::UNINITIALIZED);
            b.u2(9);
        },
        |frame| match frame {
            StackMapFrame::AppendFrame { offset_delta, locals, .. } => {
                assert_eq!(*offset_delta, 0);
                assert_eq!(items_desc(locals), "Null, Uninitialized(@9)");
            }
            other => panic!("expected append_frame, got {}", frame_kind(other)),
        },
    );
}

#[test]
fn full_frame() {
    // Locals and stack both carry explicit counts, and both lists mix widths.
    let bytes = class_with_frames(1, |b, refs| {
        b.u1(255);
        b.u2(42);
        b.u2(3);
        b.u1(item::UNINITIALIZED_THIS);
        b.u1(item::OBJECT);
        b.u2(refs.object_class);
        b.u1(item::TOP);
        b.u2(2);
        b.u1(item::UNINITIALIZED);
        b.u2(7);
        b.u1(item::INTEGER);
    });
    with_entries(&bytes, |entries| match &entries[0] {
        StackMapFrame::FullFrame { frame_type, offset_delta, locals, stack } => {
            assert_eq!(*frame_type, 255);
            assert_eq!(*offset_delta, 42);
            assert_eq!(locals.len(), 3);
            assert_eq!(items_desc(&locals[..1]), "UninitializedThis");
            assert!(item_desc(&locals[1]).starts_with("Object(#"));
            assert_eq!(items_desc(&locals[2..]), "Top");
            assert_eq!(items_desc(stack), "Uninitialized(@7), Integer");
        }
        other => panic!("expected full_frame, got {}", frame_kind(other)),
    });
}

#[test]
fn full_frame_empty() {
    // Both counts zero: nothing follows them, and the frame is still valid.
    with_frame(
        |b, _| {
            b.u1(255);
            b.u2(0);
            b.u2(0);
            b.u2(0);
        },
        |frame| match frame {
            StackMapFrame::FullFrame { offset_delta, locals, stack, .. } => {
                assert_eq!(*offset_delta, 0);
                assert!(locals.is_empty());
                assert!(stack.is_empty());
            }
            other => panic!("expected full_frame, got {}", frame_kind(other)),
        },
    );
}

#[test]
fn frame_with_bad_verification_type_rejected() {
    // A bad tag inside a frame has to fail the frame, not just be skipped.
    let bytes = class_with_frames(1, |b, _| {
        b.u1(252);
        b.u2(0);
        b.u1(42);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidVerificationTypeTag);
}

#[test]
fn frames_truncated_at_every_offset_rejected() {
    // One frame of each shape, cut at every length short of complete. Every
    // cut has to be rejected: no shape may be satisfied by a prefix of itself.
    let mut frames: Vec<Bytes> = Vec::new();

    let mut f = Bytes::new();
    f.u1(64).u1(item::OBJECT).u2(1);
    frames.push(f);

    let mut f = Bytes::new();
    f.u1(247).u2(1).u1(item::INTEGER);
    frames.push(f);

    let mut f = Bytes::new();
    f.u1(249).u2(1);
    frames.push(f);

    let mut f = Bytes::new();
    f.u1(251).u2(1);
    frames.push(f);

    let mut f = Bytes::new();
    f.u1(254).u2(1).u1(item::INTEGER).u1(item::FLOAT).u1(item::DOUBLE);
    frames.push(f);

    let mut f = Bytes::new();
    f.u1(255).u2(1).u2(1).u1(item::INTEGER).u2(1).u1(item::INTEGER);
    frames.push(f);

    for frame in &frames {
        let frame_type = frame.data[0];
        for len in 0..frame.len() {
            let cut = frame.prefix(len);
            let bytes = class_with_frames(1, |b, _| {
                b.raw(&cut);
            });
            // The builder back-patches attribute_length to what was written,
            // so a cut frame overruns the attribute rather than the file.
            with_parsed(&bytes, |result| match result {
                Err(ClassParserError::ClassParseInvalidAttributeLength) => {}
                other => panic!(
                    "frame_type {frame_type} cut to {len} of {} bytes: expected \
                     ClassParseInvalidAttributeLength, got {}",
                    frame.len(),
                    match other {
                        Ok(_) => "a successful parse".to_string(),
                        Err(e) => format!("{e:?}"),
                    }
                ),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// StackMapTable attribute
// ---------------------------------------------------------------------------

#[test]
fn stack_map_table_attribute_with_mixed_frames() {
    // Frames of different lengths back to back: each one has to leave the
    // reader exactly where the next one starts.
    let bytes = class_with_frames(5, |b, refs| {
        b.u1(3); // same_frame
        b.u1(70).u1(item::NULL); // same_locals_1_stack_item
        b.u1(252).u2(4).u1(item::INTEGER); // append
        b.u1(250).u2(2); // chop
        b.u1(255).u2(1); // full
        b.u2(1).u1(item::OBJECT).u2(refs.object_class);
        b.u2(0);
    });
    with_entries(&bytes, |entries| {
        assert_eq!(entries.len(), 5);
        assert!(matches!(entries[0], StackMapFrame::SameFrame { frame_type: 3 }));
        assert_eq!(one_stack_item(&entries[1]), (70, "Null".to_string()));
        match &entries[2] {
            StackMapFrame::AppendFrame { offset_delta, locals, .. } => {
                assert_eq!(*offset_delta, 4);
                assert_eq!(items_desc(locals), "Integer");
            }
            other => panic!("entry 2 is {}", frame_kind(other)),
        }
        match &entries[3] {
            StackMapFrame::ChopFrame { frame_type, offset_delta } => {
                assert_eq!(*frame_type, 250);
                assert_eq!(*offset_delta, 2);
            }
            other => panic!("entry 3 is {}", frame_kind(other)),
        }
        match &entries[4] {
            StackMapFrame::FullFrame { locals, stack, .. } => {
                assert!(item_desc(&locals[0]).starts_with("Object(#"));
                assert!(stack.is_empty());
            }
            other => panic!("entry 4 is {}", frame_kind(other)),
        }
    });
}

#[test]
fn stack_map_table_attribute_empty() {
    // A zero-entry table is legal and its body is just the count.
    let bytes = class_with_frames(0, |_, _| {});
    with_entries(&bytes, |entries| assert!(entries.is_empty()));
}

#[test]
fn stack_map_table_with_many_frames() {
    // number_of_entries is a u2, so more than 255 frames must work.
    let count = 300u16;
    let bytes = class_with_frames(count, |b, _| {
        for _ in 0..count {
            b.u1(0);
        }
    });
    with_entries(&bytes, |entries| assert_eq!(entries.len(), usize::from(count)));
}

#[test]
fn stack_map_table_count_larger_than_frames_rejected() {
    // The count claims three frames but the body holds one byte, so the frames
    // need more room than attribute_length gives them.
    let bytes = class_with_frames(3, |b, _| {
        b.u1(0);
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn stack_map_table_trailing_bytes_rejected() {
    // Not in the C suite: attribute_length must match what the frames use, so
    // a body longer than its declared frames is malformed (JVMS 4.7).
    let bytes = class_with_stack_map_body(|b, _| {
        b.u2(1);
        b.u1(0); // same_frame
        b.u1(0); // one byte the count does not account for
    });
    assert_rejected_with!(&bytes, ClassParserError::ClassParseInvalidAttributeLength);
}

#[test]
fn stack_map_table_in_method_code() {
    // static int m(int x) { return x > 0 ? 1 : 0; } -- needs frames at 8 and 9.
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let code_name = cb.pool.utf8("Code");
    let smt_name = cb.pool.utf8("StackMapTable");
    cb.add_method(acc::PUBLIC | acc::STATIC, "m", "(I)I", 1);
    let body = cb.methods.attr_begin(code_name);
    cb.methods.u2(1); // max_stack
    cb.methods.u2(1); // max_locals
    let code: [u8; 10] = [
        0x1a, // 0: iload_0
        0x9e, 0x00, 0x07, // 1: ifle 8
        0x04, // 4: iconst_1
        0xa7, 0x00, 0x04, // 5: goto 9
        0x03, // 8: iconst_0
        0xac, // 9: ireturn
    ];
    cb.methods.u4(code.len() as u32);
    cb.methods.raw(&code);
    cb.methods.u2(0); // exception_table_length
    cb.methods.u2(1); // attributes_count
    let smt = cb.methods.attr_begin(smt_name);
    cb.methods.u2(2);
    cb.methods.u1(8); // same_frame at 8
    cb.methods.u1(64).u1(item::INTEGER); // same_locals_1_stack_item at 9
    cb.methods.attr_end(smt);
    cb.methods.attr_end(body);

    let bytes = cb.to_bytes();
    with_entries(&bytes, |entries| {
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0], StackMapFrame::SameFrame { frame_type: 8 }));
        assert_eq!(one_stack_item(&entries[1]), (64, "Integer".to_string()));
    });
}

#[test]
fn two_stack_map_tables_in_one_code_rejected() {
    // At most one StackMapTable may appear in a Code attribute (JVMS 4.7.4).
    let mut cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let code_name = cb.pool.utf8("Code");
    let smt_name = cb.pool.utf8("StackMapTable");
    cb.add_method(acc::PUBLIC | acc::STATIC, "m", "()V", 1);
    let body = cb.methods.attr_begin(code_name);
    cb.methods.u2(0);
    cb.methods.u2(0);
    cb.methods.u4(1);
    cb.methods.u1(op::RETURN);
    cb.methods.u2(0); // exception_table_length
    cb.methods.u2(2); // attributes_count
    for _ in 0..2 {
        let smt = cb.methods.attr_begin(smt_name);
        cb.methods.u2(0);
        cb.methods.attr_end(smt);
    }
    cb.methods.attr_end(body);

    assert_rejected_with!(&cb.to_bytes(), ClassParserError::ClassParseFormatError);
}
