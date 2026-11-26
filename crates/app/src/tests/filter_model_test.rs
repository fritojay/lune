use super::*;
use crate::test_util::qml_values;

#[test]
fn groupings_qml_has_every_grouping_once() {
    let mut ids = qml_values(include_str!("../../qml/Groupings.qml"), "id");
    ids.sort_unstable();
    let mut names = Grouping::ALL.map(Grouping::name);
    names.sort_unstable();
    assert_eq!(ids, names);
}
