use super::*;

#[test]
fn a_suggested_name_is_the_first_one_not_taken() {
    let playlists = |names: &[&str]| -> Vec<Playlist> {
        (2..)
            .zip(names)
            .map(|(id, name)| Playlist {
                id: id.to_string().parse().unwrap(),
                name: (*name).to_owned(),
                track_ids: Vec::new(),
            })
            .collect()
    };
    for (names, expected) in [
        (&[][..], "Mix"),
        (&["Mix 2"][..], "Mix"),
        (&["Mix"][..], "Mix 2"),
        (&["Mix", "Mix 2", "Mix 4"][..], "Mix 3"),
        (&["mix"][..], "Mix"),
    ] {
        assert_eq!(
            suggested_name("Mix", &playlists(names)),
            expected,
            "{names:?}"
        );
    }
}
