use super::*;

#[test]
fn a_fresh_dir_skips_a_link_planted_at_its_name() {
    let parent = TempDir::new();
    let target = TempDir::new();
    let planted = parent.0.join(format!("x-{}-0", std::process::id()));
    std::os::unix::fs::symlink(&target.0, &planted).unwrap();
    let fresh = create_fresh_dir(&parent.0, "x");
    assert_ne!(fresh, planted);
    assert!(fresh.symlink_metadata().unwrap().is_dir());
    assert_eq!(std::fs::read_dir(&target.0).unwrap().count(), 0);
}
