use crate::java_utf::JavaUTF8;

pub fn is_array_descriptor(name: JavaUTF8) -> bool {
    name.as_bytes().first() == Some(&b'[')
}

pub fn valid_class_name(name: JavaUTF8) -> bool {
    !name.is_empty()
        && !name.as_bytes().starts_with(b"/") && !name.as_bytes().ends_with(b"/")
        && name.as_bytes().split(|&b| b == b'/').all(|seg| valid_segment(JavaUTF8(seg)))
}

/// Consumes one field type from the front of `d`, returning what follows, or
/// `None` if it is malformed (JVMS 4.3.2).
pub fn field_descriptor_tail(d: &[u8], depth: usize) -> Option<&[u8]> {
    match d.first()? {
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' => Some(&d[1..]),
        b'L' => {
            let end = d.iter().position(|&b| b == b';')?;
            if !valid_class_name(JavaUTF8(&d[1..end])) {
                return None;
            }
            Some(&d[end + 1..])
        }
        b'[' => {
            // "No more than 255 dimensions" (JVMS 4.3.2).
            if depth >= 255 {
                return None;
            }
            field_descriptor_tail(&d[1..], depth + 1)
        }
        _ => None,
    }
}

pub fn valid_field_descriptor(descriptor: JavaUTF8) -> bool {
    field_descriptor_tail(descriptor.as_bytes(), 0) == Some(&[][..])
}

/// The name a CONSTANT_Class may hold (JVMS 4.4.1): a binary name in internal
/// form, or — for an array type only — the array's descriptor. So a leading
/// '[' is the one case where descriptor syntax is allowed; "Ljava/lang/String;"
/// is a descriptor for a non-array type and is not a valid class name.
pub fn valid_class_entry_name(name: JavaUTF8) -> bool {
    if name.as_bytes().first() == Some(&b'[') {
        valid_field_descriptor(name)
    } else {
        valid_class_name(name)
    }
}

pub fn valid_segment(seg: JavaUTF8) -> bool {
    !seg.is_empty()                              // rejects "a//b" too
        && !seg.as_bytes().iter().any(|&b| matches!(b, b'.' | b';' | b'[' | b'/'))
}

pub fn valid_method_descriptor(descriptor: JavaUTF8) -> bool {
    let d = descriptor.as_bytes();
    if d.first() != Some(&b'(') {
        return false;
    }
    let mut tail = &d[1..];
    while let Some(t) = field_descriptor_tail(tail, 0) {
        tail = t;
    }
    if tail.first() != Some(&b')') {
        return false;
    }
    let ret = &tail[1..];
    ret == b"V" || field_descriptor_tail(ret, 0) == Some(&[][..])
}

/// An unqualified name (JVMS 4.2.2): a member's own name, not a descriptor and
/// not a qualified class name. Must be non-empty and must not contain any of
/// `. ; [ /`. A method name additionally may not contain `<` or `>` unless it
/// is exactly `<init>` or `<clinit>`, which `valid_method_name` covers.
pub fn valid_unqualified_name(name: JavaUTF8) -> bool {
    !name.is_empty()
        && !name.as_bytes().iter().any(|&b| matches!(b, b'.' | b';' | b'[' | b'/'))
}