use super::*;
use crate::test_util::{TempDir, write_wav};

#[test]
fn a_file_with_a_rate_outside_the_supported_range_does_not_open() {
    let dir = TempDir::new();
    for rate in [999, 768_001, 4_294_967_291] {
        let path = dir.join(&format!("{rate}.wav"));
        write_wav(&path, rate, 2, 0, 100);
        // InvalidFormat (not Decode) shows the demuxer accepted the header.
        assert!(
            matches!(Source::open(&path), Err(EngineError::InvalidFormat { .. })),
            "{rate}"
        );
    }
}
