use super::*;

fn controller(root: &Path) -> AstraEmuManagerController {
    AstraEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new()).unwrap()
}

#[test]
#[cfg(not(target_os = "android"))]
fn desktop_library_creates_empty_cores_directory_without_static_fvp() {
    let root = tempfile::tempdir().unwrap();
    let app = controller(root.path());
    assert!(root.path().join("cores").is_dir());
    assert!(app.registry.descriptor("astra.emu.fvp").is_none());
    assert!(app.plugin_errors.is_empty());
}

#[test]
#[cfg(not(target_os = "android"))]
fn damaged_core_keeps_manager_available_and_error_visible() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    std::fs::create_dir_all(&cores).unwrap();
    std::fs::write(
        cores.join(format!("astra_emu_broken.{}", std::env::consts::DLL_EXTENSION)),
        b"not a dynamic library",
    )
    .unwrap();
    std::fs::write(
        cores.join(format!("avcodec-62.{}", std::env::consts::DLL_EXTENSION)),
        b"dependency bytes",
    )
    .unwrap();

    let mut app = controller(root.path());
    assert!(app.plugin_errors.contains_key(&format!(
        "astra_emu_broken.{}",
        std::env::consts::DLL_EXTENSION
    )));
    assert!(!app.plugin_errors.contains_key(&format!(
        "avcodec-62.{}",
        std::env::consts::DLL_EXTENSION
    )));
    assert!(app.registry.descriptor("astra.emu.missing").is_none());
    assert!(app
        .rescan()
        .unwrap()
        .global_diagnostic
        .contains("ASTRA_EMU_FAMILY_LOAD"));
}

#[test]
#[cfg(not(target_os = "android"))]
#[ignore = "requires an explicitly built Family plugin binary and its runtime dependencies"]
fn real_core_is_loaded_from_cores_on_each_manager_restart() {
    let path = PathBuf::from(std::env::var_os("ASTRA_EMU_TEST_PLUGIN").unwrap());
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    std::fs::create_dir_all(&cores).unwrap();
    let source_dir = path.parent().expect("plugin has a parent directory");
    for entry in std::fs::read_dir(source_dir).unwrap() {
        let entry = entry.unwrap();
        if entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case(std::env::consts::DLL_EXTENSION)
            })
        {
            std::fs::copy(entry.path(), cores.join(entry.file_name())).unwrap();
        }
    }
    let file_name = path.file_name().unwrap();
    let copied = cores.join(file_name);
    assert!(copied.is_file());
    let app = controller(root.path());
    let id = app.registry.descriptors().next().unwrap().plugin_id.clone();
    assert!(app.plugin_errors.is_empty());
    assert!(app.registry.descriptor(&id).is_some());
    drop(app);
    let app = controller(root.path());
    assert!(app.plugin_errors.is_empty());
    assert!(app.registry.descriptor(&id).is_some());
}

fn add_game(controller: &mut AstraEmuManagerController, id: &str) {
    controller
        .library
        .add_game(&GameRecord {
            game_id: id.into(),
            title: "Imported folder".into(),
            user_title: None,
            location: format!("fixtures/{id}"),
            family_id: None,
            content_fingerprint: None,
            added_at_unix_ms: 1,
        })
        .unwrap();
    controller.selected_case_id = Some(id.into());
}

fn record() -> MetadataRecord {
    MetadataRecord {
        provider: MetadataProviderId::Vndb,
        remote_id: "v1".into(),
        title: "Search result".into(),
        description: None,
        alternate_titles: vec!["Alias".into()],
        developers: vec!["Developer".into()],
        tags: Vec::new(),
        release_date: Some("2026-01-01".into()),
        platforms: Vec::new(),
        engine: None,
        cover: None,
        sensitive: false,
    }
}

#[test]
fn metadata_candidates_cannot_link_another_selected_game() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    add_game(&mut app, "first");
    app.apply_metadata_payload(
        Some("first"),
        MetadataProviderId::Vndb,
        MetadataPayload::Search(vec![record()]),
    )
    .unwrap();
    let candidate = app.model().unwrap().match_reviews[0].candidate_id.clone();
    add_game(&mut app, "second");
    assert!(app.model().unwrap().match_reviews.is_empty());
    assert_eq!(
        app.accept_match(&candidate).unwrap_err(),
        "ASTRA_EMU_METADATA_MATCH_GAME_MISMATCH"
    );
    assert!(app.pending_metadata.is_empty());
    assert!(app.library.external_identities("first").unwrap().is_empty());
}

#[test]
fn metadata_search_rejects_invalid_input_and_overlapping_requests() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    add_game(&mut app, "first");
    assert_eq!(
        app.search_metadata("vndb", "Title").unwrap_err(),
        "ASTRA_EMU_METADATA_CONSENT_REQUIRED"
    );
    app.metadata_consent.insert("vndb".into(), true);
    assert!(app.search_metadata("vndb", "   ").is_err());
    assert!(app.search_metadata("vndb", &"x".repeat(257)).is_err());
    assert!(app.pending_metadata.is_empty());
    app.pending_metadata.insert(
        "existing".into(),
        PendingMetadata {
            game_id: Some("first".into()),
        },
    );
    assert_eq!(
        app.search_metadata("vndb", "Title").unwrap_err(),
        "ASTRA_EMU_METADATA_REQUEST_PENDING"
    );
    assert_eq!(
        app.unlink_identity("vndb").unwrap_err(),
        "ASTRA_EMU_METADATA_REQUEST_PENDING"
    );
    assert!(app.model().unwrap().metadata_busy);
}

#[test]
fn metadata_linked_title_persists_and_unlink_hides_stale_snapshot() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut app = controller(root.path());
        add_game(&mut app, "first");
        app.apply_metadata_payload(
            Some("first"),
            MetadataProviderId::Vndb,
            MetadataPayload::Fetch {
                record: Box::new(record()),
                cover: None,
            },
        )
        .unwrap();
        assert_eq!(app.model().unwrap().selected_title, "Search result");
        assert_eq!(app.model().unwrap().games[0].title, "Search result");
        app.search_query = "Search result".into();
        assert_eq!(app.model().unwrap().games.len(), 1);
    }
    let mut app = controller(root.path());
    app.selected_case_id = Some("first".into());
    assert_eq!(app.model().unwrap().selected_title, "Search result");
    app.library
        .set_game_user_title("first", Some("My title"))
        .unwrap();
    assert_eq!(app.model().unwrap().selected_title, "My title");
    app.library.set_game_user_title("first", None).unwrap();
    app.unlink_identity("vndb").unwrap();
    assert_eq!(app.model().unwrap().selected_title, "Imported folder");
    assert!(app
        .metadata_record("first", MetadataProviderId::Vndb)
        .unwrap()
        .is_none());
    assert_eq!(
        app.refresh_metadata("vndb").unwrap_err(),
        "ASTRA_EMU_METADATA_NOT_LINKED"
    );
}

#[test]
fn metadata_empty_search_displays_a_retriable_state() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    add_game(&mut app, "first");
    app.apply_metadata_payload(
        Some("first"),
        MetadataProviderId::Vndb,
        MetadataPayload::Search(Vec::new()),
    )
    .unwrap();
    let model = app.model().unwrap();
    assert!(!model.metadata_busy);
    assert!(model.match_reviews.is_empty());
    assert!(model.metadata_status.contains("没有找到作品"));
}
