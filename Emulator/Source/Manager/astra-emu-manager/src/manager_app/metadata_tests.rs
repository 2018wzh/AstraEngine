use super::*;

fn controller(root: &Path) -> AstraEmuManagerController {
    AstraEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new()).unwrap()
}

#[test]
#[cfg(not(target_os = "android"))]
fn desktop_library_starts_without_implicitly_registered_fvp() {
    let root = tempfile::tempdir().unwrap();
    let app = controller(root.path());
    assert!(app.library.list_installed_plugins().unwrap().is_empty());
    assert!(app.registry.descriptor("astra.emu.fvp").is_none());
}

#[test]
fn missing_installed_plugin_keeps_manager_available_and_error_visible() {
    use astra_emu_manager_core::{FamilyCapability, FamilyPluginDescriptor, VerifiedPluginInstall};
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    let descriptor = FamilyPluginDescriptor {
        family_id: "missing".into(),
        plugin_id: "astra.emu.missing".into(),
        abi_fingerprint: astra_emu_family_api::FAMILY_ABI_FINGERPRINT.into(),
        version: "1.0.0".into(),
        capabilities: vec![FamilyCapability::CpuFrame],
        supported_formats: vec!["missing.test".into()],
    };
    let record = VerifiedPluginInstall::from_verified_descriptor(
        descriptor,
        root.path().join("missing.dll").to_str().unwrap().into(),
        1,
    )
    .unwrap();
    app.library.install_verified_plugin(&record).unwrap();
    drop(app);
    let mut app = controller(root.path());
    assert!(app.registry.descriptor("astra.emu.missing").is_none());
    assert!(app.plugin_errors.contains_key("astra.emu.missing"));
    assert!(app
        .rescan()
        .unwrap()
        .global_diagnostic
        .contains("astra.emu.missing"));
    assert_eq!(app.library.list_installed_plugins().unwrap().len(), 1);
}

#[test]
#[ignore = "requires an explicitly built Family plugin binary"]
fn changed_plugin_requires_explicit_reinstall_without_blocking_manager() {
    use astra_emu_manager_core::{LoadedFamilyPlugin, VerifiedPluginInstall};
    let path = PathBuf::from(std::env::var_os("ASTRA_EMU_TEST_PLUGIN").unwrap());
    let plugin = LoadedFamilyPlugin::load(&path).unwrap();
    let mut descriptor = plugin.manager_descriptor().clone();
    let id = descriptor.plugin_id.clone();
    descriptor.version = "obsolete-install".into();
    let record = VerifiedPluginInstall::from_verified_descriptor(
        descriptor,
        path.to_str().unwrap().into(),
        1,
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    app.library.install_verified_plugin(&record).unwrap();
    drop(app);
    let mut app = controller(root.path());
    assert!(app.registry.descriptor(&id).is_none());
    assert_eq!(
        app.plugin_errors[&id],
        "ASTRA_EMU_PLUGIN_INSTALL_DESCRIPTOR_CHANGED"
    );
    app.install_family_plugin(&path).unwrap();
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
