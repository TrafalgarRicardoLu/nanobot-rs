use super::is_allowed;

#[test]
fn empty_allow_list_denies_access() {
    assert!(!is_allowed(&[], "user-1"));
}

#[test]
fn wildcard_and_segment_matching_are_allowed() {
    assert!(is_allowed(&["*".to_string()], "user-1"));
    assert!(is_allowed(&["thread-9".to_string()], "user-1|thread-9"));
}
