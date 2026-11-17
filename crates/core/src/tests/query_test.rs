use super::*;

#[test]
fn fts_query_quotes_every_word_as_a_prefix() {
    assert_eq!(
        fts_query("abbey road").as_deref(),
        Some("\"abbey\"* \"road\"*")
    );
    assert_eq!(fts_query("  ").as_deref(), None);
    assert_eq!(
        fts_query("a OR \"b\" NEAR(c)").as_deref(),
        Some("\"a\"* \"OR\"* \"b\"* \"NEAR(c)\"*")
    );
    assert_eq!(fts_query("\"\"").as_deref(), None);
}

#[test]
fn search_treats_operators_as_text() {
    let conn = crate::db::open_in_memory().unwrap();
    for text in ["OR", "NEAR(", "title:x", "-a", "*", "\"", "a AND"] {
        assert!(search(&conn, text, 10).is_ok(), "{text}");
    }
}
