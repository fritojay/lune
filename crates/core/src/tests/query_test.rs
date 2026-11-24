use proptest::prelude::*;

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

/// Text full of FTS5 syntax: operators, quotes, columns, and prefixes.
fn arb_search_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        prop::sample::select(
            &[
                "OR", "AND", "NOT", "NEAR(", ")", "title:", "-", "*", "\"", "^", "{", " "
            ][..]
        )
        .prop_map(str::to_owned),
        any::<String>(),
    ];
    prop::collection::vec(piece, 0..6).prop_map(|pieces| pieces.concat())
}

proptest! {
    #[test]
    fn search_treats_any_text_as_words_to_match(text in arb_search_text()) {
        let words: Vec<String> = text
            .split_whitespace()
            .map(|w| w.replace('"', ""))
            .filter(|w| !w.is_empty())
            .collect();
        let quoted = fts_query(&text);
        prop_assert_eq!(
            quoted.as_deref().map_or(0, |q| q.split_whitespace().count()),
            words.len()
        );
        for (term, word) in quoted.iter().flat_map(|q| q.split_whitespace()).zip(&words) {
            prop_assert_eq!(term, format!("\"{word}\"*"));
        }
        let conn = crate::db::open_in_memory().unwrap();
        prop_assert!(search(&conn, &text, 10).is_ok());
    }
}
