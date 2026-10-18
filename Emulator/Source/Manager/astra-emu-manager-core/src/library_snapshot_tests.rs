use super::*;
use crate::{MetadataSnapshotState, PlaySessionEndReason};

fn game(game_id: &str, title: &str) -> GameRecord {
    GameRecord {
        game_id: game_id.into(),
        title: title.into(),
        user_title: None,
        location: format!("location-{game_id}"),
        family_id: Some("fvp".into()),
        content_fingerprint: None,
        added_at_unix_ms: 1,
    }
}

fn identity(game_id: &str, provider: &str, remote_id: &str) -> ExternalIdentityRecord {
    ExternalIdentityRecord {
        game_id: game_id.into(),
        provider: provider.into(),
        remote_id: remote_id.into(),
        provenance: "manual".into(),
        linked_at_unix_ms: 2,
    }
}

fn metadata(
    identity: &ExternalIdentityRecord,
    state: MetadataSnapshotState,
    metadata_json: &str,
) -> MetadataSnapshotRecord {
    MetadataSnapshotRecord {
        game_id: identity.game_id.clone(),
        provider: identity.provider.clone(),
        remote_id: identity.remote_id.clone(),
        metadata_json: metadata_json.into(),
        fetched_at_unix_ms: 3,
        state,
    }
}

fn finish_session(library: &mut Library, game_id: &str, start: i64, end: i64) {
    let session = library.start_play_session(game_id, start).unwrap();
    library
        .end_play_session(&session, end, PlaySessionEndReason::Leave)
        .unwrap();
}

#[test]
fn library_snapshot_aggregates_sessions_without_multiplying_provider_rows() {
    let mut library = Library::in_memory().unwrap();
    let mut a = game("game-a", "alpha");
    a.user_title = Some("User title".into());
    let b = game("game-b", "Beta");
    let c = game("game-c", "ALPHA");
    let d = game("game-d", "delta");
    for record in [&d, &b, &c, &a] {
        library.add_game(record).unwrap();
    }
    finish_session(&mut library, "game-a", 100, 400);
    finish_session(&mut library, "game-a", 800, 1_100);
    finish_session(&mut library, "game-a", 1_200, 1_250);
    finish_session(&mut library, "game-d", 1_000, 900);
    library.start_play_session("game-b", 2_000).unwrap();

    let a_vndb = identity("game-a", "vndb", "v1");
    let a_bangumi = identity("game-a", "bangumi", "10");
    let a_other = identity("game-a", "other", "entry-a");
    let b_vndb = identity("game-b", "vndb", "v2");
    for record in [&a_vndb, &a_other, &b_vndb, &a_bangumi] {
        library.set_external_identity(record).unwrap();
    }
    let records = [
        metadata(
            &a_vndb,
            MetadataSnapshotState::Fresh,
            "{\n  \"title\": \"A\"\n}",
        ),
        metadata(
            &a_bangumi,
            MetadataSnapshotState::Stale,
            r#"{"title":"A old"}"#,
        ),
        metadata(
            &a_other,
            MetadataSnapshotState::Failed,
            r#"{"error":"unavailable"}"#,
        ),
        metadata(&b_vndb, MetadataSnapshotState::Fresh, r#"{"title":"B"}"#),
    ];
    for record in &records {
        library.set_metadata_snapshot(record).unwrap();
    }

    let snapshot = library.library_snapshot().unwrap();
    assert_eq!(snapshot.games, vec![a, c, b, d]);
    assert_eq!(
        snapshot.play_stats,
        BTreeMap::from([
            (
                "game-a".into(),
                PlayStats {
                    total_duration_ms: 650,
                    last_played_unix_ms: Some(1_200),
                    session_count: 3,
                },
            ),
            ("game-b".into(), PlayStats::default()),
            ("game-c".into(), PlayStats::default()),
            (
                "game-d".into(),
                PlayStats {
                    total_duration_ms: 0,
                    last_played_unix_ms: Some(1_000),
                    session_count: 1,
                },
            ),
        ]),
    );
    assert_eq!(
        snapshot.identities,
        BTreeMap::from([
            ("game-a".into(), vec![a_bangumi, a_other, a_vndb]),
            ("game-b".into(), vec![b_vndb]),
        ]),
    );
    assert_eq!(
        snapshot.metadata,
        records
            .into_iter()
            .map(|record| ((record.game_id.clone(), record.provider.clone()), record))
            .collect(),
    );
}

#[test]
fn library_snapshot_tracks_finished_and_abandoned_sessions() {
    let mut library = Library::in_memory().unwrap();
    library.add_game(&game("played", "Played")).unwrap();
    let session = library.start_play_session("played", 100).unwrap();
    assert_eq!(
        library.library_snapshot().unwrap().play_stats["played"],
        PlayStats::default(),
    );

    library
        .end_play_session(&session, 450, PlaySessionEndReason::Shutdown)
        .unwrap();
    library.start_play_session("played", 800).unwrap();
    assert_eq!(
        library.library_snapshot().unwrap().play_stats["played"],
        PlayStats {
            total_duration_ms: 350,
            last_played_unix_ms: Some(100),
            session_count: 1,
        },
    );
    assert_eq!(library.settle_abandoned_sessions(1_000).unwrap(), 1);
    assert_eq!(
        library.library_snapshot().unwrap().play_stats["played"],
        PlayStats {
            total_duration_ms: 350,
            last_played_unix_ms: Some(800),
            session_count: 2,
        },
    );
}

#[test]
fn library_snapshot_reflects_metadata_changes_unlinks_and_deletion() {
    let mut library = Library::in_memory().unwrap();
    library.add_game(&game("one", "One")).unwrap();
    let two = game("two", "Two");
    library.add_game(&two).unwrap();
    finish_session(&mut library, "one", 100, 200);
    let vndb = identity("one", "vndb", "v1");
    let bangumi = identity("one", "bangumi", "10");
    let other = identity("two", "vndb", "v3");
    let initial = metadata(
        &vndb,
        MetadataSnapshotState::Fresh,
        r#"{"title":"Original"}"#,
    );
    let bangumi_metadata = metadata(&bangumi, MetadataSnapshotState::Stale, "{}");
    let other_metadata = metadata(&other, MetadataSnapshotState::Fresh, r#"{"title":"Other"}"#);
    for record in [&vndb, &bangumi, &other] {
        library.set_external_identity(record).unwrap();
    }
    for record in [&initial, &bangumi_metadata, &other_metadata] {
        library.set_metadata_snapshot(record).unwrap();
    }
    let before = library.library_snapshot().unwrap();

    let relinked = identity("one", "vndb", "v2");
    library.set_external_identity(&relinked).unwrap();
    let relinked_snapshot = library.library_snapshot().unwrap();
    let key = ("one".into(), "vndb".into());
    assert_eq!(relinked_snapshot.metadata[&key], initial);
    assert_eq!(
        relinked_snapshot.identities["one"],
        vec![bangumi, relinked.clone()]
    );

    let mut updated = metadata(
        &relinked,
        MetadataSnapshotState::Failed,
        r#"{"title":"Updated"}"#,
    );
    updated.fetched_at_unix_ms = 10;
    library.set_metadata_snapshot(&updated).unwrap();
    assert!(library.remove_external_identity("one", "vndb").unwrap());
    library
        .connection
        .execute(
            "DELETE FROM metadata_snapshot WHERE game_id=?1 AND provider=?2",
            ["one", "bangumi"],
        )
        .unwrap();
    let changed = library.library_snapshot().unwrap();
    assert_eq!(before.metadata[&key], initial);
    assert_eq!(changed.metadata[&key], updated);
    assert!(
        !changed
            .metadata
            .contains_key(&("one".into(), "bangumi".into()))
    );
    assert_eq!(
        changed.metadata[&("two".into(), "vndb".into())],
        other_metadata
    );
    assert_eq!(changed.identities["one"].len(), 1);
    assert_eq!(changed.identities["one"][0].provider, "bangumi");

    assert!(library.remove_game("one").unwrap());
    let removed = library.library_snapshot().unwrap();
    assert_eq!(removed.games, vec![two]);
    assert_eq!(
        removed.play_stats,
        BTreeMap::from([("two".into(), PlayStats::default())])
    );
    assert_eq!(
        removed.identities,
        BTreeMap::from([("two".into(), vec![other])])
    );
    assert_eq!(
        removed.metadata,
        BTreeMap::from([(("two".into(), "vndb".into()), other_metadata)]),
    );
}

#[test]
fn library_snapshot_rejects_invalid_metadata_state_and_releases_failed_read() {
    let mut library = Library::in_memory().unwrap();
    library.add_game(&game("one", "One")).unwrap();
    let record = metadata(
        &identity("one", "vndb", "v1"),
        MetadataSnapshotState::Fresh,
        r#"{"title":"Valid"}"#,
    );
    library.set_metadata_snapshot(&record).unwrap();
    library
        .connection
        .execute("UPDATE metadata_snapshot SET state='unknown'", [])
        .unwrap();
    assert!(matches!(
        library.library_snapshot(),
        Err(LibraryError::Sqlite(
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, _,)
        )),
    ));
    assert!(matches!(
        library.metadata_snapshot("one", "vndb"),
        Err(LibraryError::Sqlite(
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, _,)
        )),
    ));

    library.set_metadata_snapshot(&record).unwrap();
    let mut invalid = record.clone();
    invalid.metadata_json = "not-json".into();
    assert!(matches!(
        library.set_metadata_snapshot(&invalid),
        Err(LibraryError::InvalidMetadata),
    ));
    assert_eq!(
        library.library_snapshot().unwrap().metadata[&("one".into(), "vndb".into())],
        record,
    );
}

#[test]
fn library_snapshot_preserves_sqlite_aggregate_overflow_errors() {
    let mut library = Library::in_memory().unwrap();
    library.add_game(&game("large", "Large")).unwrap();
    let other = game("other", "Other");
    library.add_game(&other).unwrap();
    finish_session(&mut library, "large", i64::MIN, 0);
    finish_session(&mut library, "large", 10, 11);
    assert!(matches!(
        library.library_snapshot(),
        Err(LibraryError::Sqlite(rusqlite::Error::SqliteFailure(_, _))),
    ));
    assert!(library.remove_game("large").unwrap());
    let snapshot = library.library_snapshot().unwrap();
    assert_eq!(snapshot.games, vec![other]);
    assert_eq!(snapshot.play_stats["other"], PlayStats::default());
}
