use crate::common::*;

#[test]
fn builder_produces_a_well_formed_header() {
    let cb = ClassBuilder::new(DEFAULT_MAJOR_VERSION);
    let bytes = cb.to_bytes();
    assert_eq!(&bytes[0..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
    assert_eq!(&bytes[4..6], &[0, 0]);
    assert_eq!(&bytes[6..8], &[0, 52]);
    assert_eq!(&bytes[8..10], &[0, 5]); // constant_pool_count
}

#[test]
fn descriptor_slots() {
    assert_eq!(descriptor_arg_slots("()V"), 0);
    assert_eq!(descriptor_arg_slots("(I)I"), 1);
    assert_eq!(descriptor_arg_slots("(JD)V"), 4);
    assert_eq!(descriptor_arg_slots("([Ljava/lang/String;)V"), 1);
    assert_eq!(descriptor_arg_slots("(IJD)I"), 5);
}

#[test]
fn fixtures_are_present() {
    for name in ALL_FIXTURES {
        assert!(!fixture(name).is_empty(), "{name} is empty");
    }
}
