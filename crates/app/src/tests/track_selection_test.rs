use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;

#[derive(Debug, Clone)]
enum Op {
    Click {
        row: usize,
        extend: bool,
        toggle: bool,
    },
    Keep(usize),
    Range(usize, usize),
    Clear,
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..12usize, any::<bool>(), any::<bool>())
            .prop_map(|(row, extend, toggle)| Op::Click { row, extend, toggle }),
        1 => (0..12usize).prop_map(Op::Keep),
        1 => (0..12usize, 0..5usize).prop_map(|(start, count)| Op::Range(start, count)),
        1 => Just(Op::Clear),
    ]
}

proptest! {
    #[test]
    fn each_click_changes_the_selection_as_file_managers_do(
        ops in prop::collection::vec(arb_op(), 0..30),
    ) {
        let mut rows = Rows::default();
        let mut anchor: Option<usize> = None;
        for op in &ops {
            let before: BTreeSet<usize> = rows.rows().collect();
            let range = |from: usize, to: usize| -> BTreeSet<usize> {
                (from.min(to)..=from.max(to)).collect()
            };
            let expected: BTreeSet<usize> = match *op {
                Op::Click { row, extend: true, toggle } if anchor.is_some() => {
                    let range = range(anchor.unwrap(), row);
                    if toggle { before.union(&range).copied().collect() } else { range }
                }
                Op::Click { row, toggle: true, .. } => {
                    anchor = Some(row);
                    before.symmetric_difference(&BTreeSet::from([row])).copied().collect()
                }
                Op::Click { row, .. } => {
                    anchor = Some(row);
                    BTreeSet::from([row])
                }
                Op::Keep(row) if before.contains(&row) => before.clone(),
                Op::Keep(row) => {
                    anchor = Some(row);
                    BTreeSet::from([row])
                }
                Op::Range(start, count) => {
                    anchor = Some(start);
                    (start..start + count).collect()
                }
                Op::Clear => {
                    anchor = None;
                    BTreeSet::new()
                }
            };
            match *op {
                Op::Click { row, extend, toggle } => rows.click(row, extend, toggle),
                Op::Keep(row) => rows.keep(row),
                Op::Range(start, count) => rows.select_range(start, count),
                Op::Clear => rows.clear(),
            }
            prop_assert_eq!(rows.rows().collect::<BTreeSet<_>>(), expected, "{:?}", op);
            prop_assert_eq!(rows.len(), rows.rows().count());
            for row in 0..14 {
                prop_assert_eq!(rows.contains(row), rows.rows().any(|r| r == row));
            }
        }
    }
}
