use super::*;

fn controller(root: &Path) -> AstraEmuManagerController {
    AstraEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new()).unwrap()
}

#[test]
#[cfg(not(target_os = "android"))]
fn damaged_core_keeps_manager_available_and_error_visible() {
    let root = tempfile::tempdir().unwrap();
    let core = root.path().join("cores").join("astra_emu_broken");
    std::fs::create_dir_all(&core).unwrap();
    std::fs::write(
        core.join(format!(
            "astra_emu_broken.{}",
            std::env::consts::DLL_EXTENSION
        )),
        b"not a dynamic library",
    )
    .unwrap();
    std::fs::write(
        core.join(format!("avcodec-62.{}", std::env::consts::DLL_EXTENSION)),
        b"dependency bytes",
    )
    .unwrap();

    let mut app = controller(root.path());
    app.prepare_library().unwrap();
    super::probe_state_tests::settle_background(&mut app);
    assert!(app.plugin_errors.contains_key("astra_emu_broken"));
    assert!(!app.plugin_errors.contains_key("avcodec-62"));
    assert!(app.registry.descriptor("astra.emu.missing").is_none());
    assert!(
        app.model()
            .unwrap()
            .notices
            .items
            .iter()
            .any(|notice| notice.code.contains("ASTRA_EMU_FAMILY_LOAD"))
    );
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
    let candidate = app.model().unwrap().selection.matches[0].id.clone();
    add_game(&mut app, "second");
    assert!(app.model().unwrap().selection.matches.is_empty());
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
        "ASTRA_EMU_NETWORK_PERMISSION_REQUIRED"
    );
    app.network_allowed = true;
    assert!(app.search_metadata("vndb", "   ").is_err());
    assert!(app.search_metadata("vndb", &"x".repeat(257)).is_err());
    assert!(app.pending_metadata.is_empty());
    app.pending_metadata.insert(
        "existing".into(),
        PendingMetadata {
            game_id: Some("first".into()),
            core: None,
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
    assert!(app.model().unwrap().selection.metadata_busy);
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
        assert_eq!(app.model().unwrap().selection.game.title, "Search result");
        assert_eq!(app.model().unwrap().library.games[0].title, "Search result");
        app.search_query = "Search result".into();
        assert_eq!(app.model().unwrap().library.games.len(), 1);
    }
    let mut app = controller(root.path());
    app.selected_case_id = Some("first".into());
    assert_eq!(app.model().unwrap().selection.game.title, "Search result");
    app.library
        .set_game_user_title("first", Some("My title"))
        .unwrap();
    assert_eq!(app.model().unwrap().selection.game.title, "My title");
    app.library.set_game_user_title("first", None).unwrap();
    app.unlink_identity("vndb").unwrap();
    assert_eq!(app.model().unwrap().selection.game.title, "Imported folder");
    assert!(
        app.metadata_record("first", MetadataProviderId::Vndb)
            .unwrap()
            .is_none()
    );
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
    assert!(!model.selection.metadata_busy);
    assert!(model.selection.matches.is_empty());
    assert!(model.selection.metadata_status.contains("没有找到作品"));
}

#[test]
fn network_permission_gates_metadata_and_translation_and_persists() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut app = controller(root.path());
        add_game(&mut app, "first");
        assert!(!app.network_allowed);
        assert_eq!(
            app.search_metadata("bangumi", "Title").unwrap_err(),
            "ASTRA_EMU_NETWORK_PERMISSION_REQUIRED"
        );
        assert!(app.pending_metadata.is_empty());
        let profile = TranslationProfile {
            model: "model".into(),
            ..TranslationProfile::default()
        };
        app.library.set_translation_profile(&profile).unwrap();
        app.library
            .set_app_state(TRANSLATION_ENABLED_KEY, &true)
            .unwrap();
    }
    let mut app = controller(root.path());
    assert!(app.translation_enabled);
    assert!(app.launch_translation_profile().unwrap().is_none());
    assert_eq!(
        app.set_translation_enabled(true).unwrap_err(),
        "ASTRA_EMU_NETWORK_PERMISSION_REQUIRED"
    );
    app.set_network_allowed(true).unwrap();
    assert!(app.launch_translation_profile().unwrap().is_some());
    drop(app);
    let app = controller(root.path());
    assert!(app.network_allowed);
    assert!(app.model().unwrap().network.allowed);
}
