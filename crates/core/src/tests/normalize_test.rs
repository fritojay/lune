use super::key;

#[test]
fn keys_fold_case_composition_and_whitespace() {
    assert_eq!(key("Beyonc\u{e9}"), key("BEYONCE\u{301}"));
    assert_eq!(key("STRASSE"), key("stra\u{df}e"));
    assert_eq!(key("  The\tWall \n"), "the wall");
    assert_eq!(key(""), "");
}
