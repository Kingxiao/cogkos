use super::falkor::*;
use uuid::Uuid;

#[test]
fn test_validate_uuid_valid() {
    let id = Uuid::new_v4();
    assert!(validate_uuid(&id).is_ok());
}

#[test]
fn test_validate_uuid_rejects_injection() {
    assert!(validate_uuid(&"not-a-uuid").is_err());
    assert!(validate_uuid(&"'; DROP GRAPH --").is_err());
    assert!(validate_uuid(&"12345").is_err());
}

#[test]
fn test_cypher_escape_backslash() {
    assert_eq!(cypher_escape("hello\\world"), "hello\\\\world");
}

#[test]
fn test_cypher_escape_single_quote() {
    assert_eq!(cypher_escape("it's"), "it\\'s");
}

#[test]
fn test_cypher_escape_combined() {
    assert_eq!(cypher_escape("a\\b'c"), "a\\\\b\\'c");
}

#[test]
fn test_cypher_escape_clean_string() {
    assert_eq!(cypher_escape("hello world"), "hello world");
}

#[test]
fn test_validate_relation_valid() {
    assert!(validate_relation("CONTAINS").is_ok());
    assert!(validate_relation("derived_from").is_ok());
    assert!(validate_relation("REL123").is_ok());
}

#[test]
fn test_validate_relation_rejects_empty() {
    assert!(validate_relation("").is_err());
}

#[test]
fn test_validate_relation_rejects_special_chars() {
    assert!(validate_relation("REL-TYPE").is_err());
    assert!(validate_relation("REL TYPE").is_err());
    assert!(validate_relation("rel;DROP").is_err());
    assert!(validate_relation("rel'inject").is_err());
}
