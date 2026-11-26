use super::*;

#[test]
fn every_grouping_is_listed_once_in_order_with_only_the_shown_one_current() {
    for shown in Grouping::ALL {
        let sources = sources(shown);
        let groupings: Vec<Grouping> = sources.iter().map(|s| s.grouping).collect();
        assert_eq!(groupings, Grouping::ALL);
        let current: Vec<Grouping> = sources
            .iter()
            .filter(|s| s.current)
            .map(|s| s.grouping)
            .collect();
        assert_eq!(current, [shown]);
    }
}
