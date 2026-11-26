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

#[test]
fn a_new_current_source_changes_only_the_old_and_new_rows() {
    for from in Grouping::ALL {
        for to in Grouping::ALL {
            let mut expected: Vec<i32> = if from == to {
                Vec::new()
            } else {
                [from, to].map(|g| g as i32).into()
            };
            expected.sort_unstable();
            assert_eq!(
                changed_rows(&sources(from), &sources(to)),
                expected,
                "{from:?} {to:?}"
            );
        }
    }
}

#[test]
fn grouping_bar_qml_has_a_title_for_every_grouping_once() {
    let mut names: Vec<&str> = include_str!("../../qml/GroupingBar.qml")
        .lines()
        .filter_map(|line| line.trim().split_once("\": qsTr("))
        .map(|(name, _)| name.trim_start_matches('"'))
        .collect();
    names.sort_unstable();
    let mut expected = Grouping::ALL.map(Grouping::name);
    expected.sort_unstable();
    assert_eq!(names, expected);
}
