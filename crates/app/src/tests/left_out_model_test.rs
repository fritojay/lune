use super::*;

#[test]
fn each_reason_is_said_with_its_cause() {
    for (why, said) in [
        (
            LeftOutReason::Tags("failed to parse Mpeg file".into()),
            "Tags couldn't be read: failed to parse Mpeg file",
        ),
        (LeftOutReason::Link, "Linked file; links aren't followed"),
        (LeftOutReason::NonUtf8Name, "Name isn't valid UTF-8"),
        (
            LeftOutReason::Unreadable("Permission denied (os error 13)".into()),
            "Couldn't be opened: Permission denied (os error 13)",
        ),
    ] {
        assert_eq!(reason(&why), said);
    }
}
