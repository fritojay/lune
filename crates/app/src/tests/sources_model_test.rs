use super::*;

#[test]
fn every_grouping_then_playlists_is_listed_once_with_only_the_shown_one_current() {
    let tabs: Vec<Tab> = Grouping::ALL
        .map(Tab::Grouping)
        .into_iter()
        .chain([Tab::Playlists])
        .collect();
    let playlists = [None, Some("7".parse().unwrap())].map(|id| Shown::Playlist { id });
    for shown in Grouping::ALL
        .map(Shown::Library)
        .into_iter()
        .chain(playlists)
    {
        let sources = sources(shown);
        assert_eq!(sources.iter().map(|s| s.tab).collect::<Vec<_>>(), tabs);
        let current: Vec<Tab> = sources
            .iter()
            .filter(|s| s.current)
            .map(|s| s.tab)
            .collect();
        let expected = match shown {
            Shown::Library(grouping) => Tab::Grouping(grouping),
            Shown::Playlist { .. } => Tab::Playlists,
        };
        assert_eq!(current, [expected], "{shown:?}");
        assert_eq!(kind(expected.source()), kind(shown));
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
                changed_rows(&sources(Shown::Library(from)), &sources(Shown::Library(to))),
                expected,
                "{from:?} {to:?}"
            );
        }
    }
}

#[test]
fn grouping_bar_qml_has_a_title_for_every_tab_once() {
    let mut names: Vec<&str> = include_str!("../../qml/GroupingBar.qml")
        .lines()
        .filter_map(|line| line.trim().split_once("\": qsTr("))
        .map(|(name, _)| name.trim_start_matches('"'))
        .collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = sources(Shown::default())
        .iter()
        .map(|s| s.tab.id())
        .collect();
    expected.sort_unstable();
    assert_eq!(names, expected);
}
