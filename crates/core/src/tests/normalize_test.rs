use proptest::prelude::*;
use unicode_normalization::{UnicodeNormalization, is_nfc};

use super::key;

#[test]
fn keys_fold_case_composition_and_whitespace() {
    assert_eq!(key("Beyonc\u{e9}"), key("BEYONCE\u{301}"));
    assert_eq!(key("STRASSE"), key("stra\u{df}e"));
    assert_eq!(key("  The\tWall \n"), "the wall");
    assert_eq!(key(""), "");
}

/// Random words, and ones whose case folding decomposes or expands them.
fn arb_word() -> impl Strategy<Value = String> {
    prop_oneof![
        "\\PZ+",
        prop::sample::select(&["\u{1f0}", "\u{390}", "\u{df}", "\u{130}", "Beyonc\u{e9}"][..])
            .prop_map(str::to_owned),
    ]
}

proptest! {
    #[test]
    fn keys_are_composed_stable_and_ignore_spacing_and_composition(
        words in prop::collection::vec(arb_word(), 0..4),
        gaps in prop::collection::vec(prop::sample::select(&[" ", "  ", "\t", "\n", "\u{a0}"][..]), 5),
    ) {
        let spaced: String = words.iter().zip(&gaps).map(|(w, g)| format!("{g}{w}")).collect();
        let k = key(&words.join(" "));
        prop_assert_eq!(key(&spaced), k.clone());
        prop_assert_eq!(key(&spaced.nfd().collect::<String>()), k.clone());
        prop_assert_eq!(key(&k), k.clone());
        prop_assert!(is_nfc(&k));
        prop_assert!(!k.starts_with(' ') && !k.ends_with(' ') && !k.contains("  "));
    }
}
