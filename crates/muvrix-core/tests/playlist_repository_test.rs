use muvrix_core::database::DatabaseManager;

#[test]
fn playlist_crud_ordering_and_cascade() {
    let db = DatabaseManager::open_in_memory().expect("db");
    let id = db.create_playlist("Road Trip").expect("create");
    db.rename_playlist(id, "Road Trip 2026").expect("rename");

    db.add_playlist_entry(id, "/media/a.mp4", Some("A"), 1_000)
        .expect("add a");
    db.add_playlist_entry(id, "https://example.test/b.m3u8", Some("B"), 2_000)
        .expect("add b");
    db.add_playlist_entry(id, "/media/c.mp4", Some("C"), 3_000)
        .expect("add c");

    let lists = db.list_playlists().expect("list");
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].name, "Road Trip 2026");
    assert_eq!(lists[0].item_count, 3);

    let mut entries = db.list_playlist_entries(id).expect("entries");
    let c_id = entries[2].id;
    assert_eq!(entries[0].position, 0);
    assert_eq!(entries[1].position, 1);
    assert_eq!(entries[2].position, 2);

    db.reorder_playlist_entry(id, c_id, 0).expect("reorder");
    entries = db.list_playlist_entries(id).expect("reordered");
    assert_eq!(entries[0].id, c_id);
    assert_eq!(entries[0].position, 0);
    assert_eq!(entries[1].position, 1);
    assert_eq!(entries[2].position, 2);

    let remove_id = entries[1].id;
    db.remove_playlist_entry(id, remove_id).expect("remove");
    entries = db.list_playlist_entries(id).expect("renumbered");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].position, 0);
    assert_eq!(entries[1].position, 1);

    db.clear_playlist(id).expect("clear");
    assert!(db.list_playlist_entries(id).expect("empty").is_empty());
    db.add_playlist_entry(id, "/media/d.mp4", None, 0)
        .expect("add after clear");

    db.delete_playlist(id).expect("delete");
    assert!(db.list_playlists().expect("final list").is_empty());
    assert!(db
        .list_playlist_entries(id)
        .expect("cascade entries")
        .is_empty());
}
