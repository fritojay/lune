use unicode_normalization::UnicodeNormalization;

/// Keys are only for comparison. Display strings are stored separately, unchanged.
pub fn key(s: &str) -> String {
    let nfc: String = s.nfc().collect();
    let folded = caseless::default_case_fold_str(&nfc);
    let mut out = String::with_capacity(folded.len());
    for word in folded.nfc().collect::<String>().split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
#[path = "tests/normalize_test.rs"]
mod tests;
