use super::*;

const ROLES: &[Role<i32>] = &[
    ("same", |n| QVariant::from(n)),
    ("doubled", |n| QVariant::from(&(n * 2))),
];

#[test]
fn roles_are_numbered_from_the_user_role_in_table_order() {
    assert_eq!(role_id(ROLES, "same"), 0x0100);
    assert_eq!(role_id(ROLES, "doubled"), 0x0101);
    let names = role_names(ROLES);
    assert_eq!(names.len(), ROLES.len() as isize);
    for (name, _) in ROLES {
        let id = role_id(ROLES, name);
        assert_eq!(
            names.get(&id).map(|n| n.to_string()),
            Some(name.to_string())
        );
    }
}

#[test]
fn data_reads_the_role_of_the_row() {
    let doubled = role_id(ROLES, "doubled");
    assert_eq!(data(ROLES, Some(&21), doubled).value::<i32>(), Some(42));
}

#[test]
fn a_missing_row_or_unknown_role_has_no_data() {
    for (row, role) in [
        (None, 0x0100),
        (Some(&1), 0x0100 - 1),
        (Some(&1), 0x0100 + 2),
        (Some(&1), i32::MIN),
        (Some(&1), -1),
    ] {
        assert!(data(ROLES, row, role).is_null(), "{row:?} {role}");
    }
}

#[test]
fn the_row_count_is_the_length_capped_at_i32_max() {
    assert_eq!(row_count(3, &QModelIndex::default()), 3);
    assert_eq!(row_count(usize::MAX, &QModelIndex::default()), i32::MAX);
}
