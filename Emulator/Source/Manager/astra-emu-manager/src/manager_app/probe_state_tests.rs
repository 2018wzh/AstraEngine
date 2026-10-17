use super::*;

use crate::test_native::{self, Fixture};

fn provider(
    plugin_id: &'static str,
    confidence_permyriad: u16,
) -> astra_emu_manager_core::LoadedFamilyPlugin {
    provider_with_marker(plugin_id, confidence_permyriad, "match")
}
fn provider_with_marker(
    plugin_id: &'static str,
    confidence_permyriad: u16,
    marker: &'static str,
) -> astra_emu_manager_core::LoadedFamilyPlugin {
    let mut fixture = Fixture::new(plugin_id, "fixture", "fixture");
    fixture.confidence = u32::from(confidence_permyriad);
    fixture.marker = Some(marker);
    test_native::install(if plugin_id == "test-a" { 0 } else { 1 }, fixture)
}

fn controller(root: &Path) -> AstraEmuManagerController {
    AstraEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new()).unwrap()
}

fn game(root: &Path) -> PathBuf {
    let path = root.join("game");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("match"), b"match").unwrap();
    std::fs::canonicalize(path).unwrap()
}

fn scan_once(app: &mut AstraEmuManagerController, path: &Path) {
    app.scan_paths(&[path.to_owned()]).unwrap();
}

#[test]
fn imported_game_selection_loads_its_input_mapping() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    app.input_mapping.deadzone = GamepadDeadzone::High;
    let model = app.add_game_directory(&path).unwrap();
    assert_eq!(model.selected_case_id, Some(path_id(&path)));
    assert_eq!(app.input_mapping.deadzone, default_vn_preset().deadzone);
}

#[test]
fn unsupported_import_reports_the_selected_directory_with_existing_games() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    app.add_game_directory(&path).unwrap();
    let unsupported = root.path().join("unsupported");
    std::fs::create_dir(&unsupported).unwrap();
    let model = app.add_game_directory(&unsupported).unwrap();
    assert_eq!(model.games.len(), 1);
    assert!(model.global_diagnostic.contains("所选目录中未发现"));
}

#[test]
fn scanning_stops_inside_a_recognized_game() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let assets = path.join("data").join("pack");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("match"), b"archive marker").unwrap();
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    let imported = app.scan_paths(&[root.path().into(), assets]).unwrap();
    assert_eq!(imported, vec![path_id(&path)]);
    assert_eq!(app.library.list_games().unwrap().len(), 1);
}

#[test]
fn importing_a_parent_keeps_the_existing_game_id_and_settings() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let alias = PathBuf::from(format!("{}{}", path.display(), std::path::MAIN_SEPARATOR));
    let id = path_id(&alias);
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    app.library
        .add_game(&GameRecord {
            game_id: id.clone(),
            title: "game".into(),
            user_title: Some("My game".into()),
            location: alias.to_str().unwrap().into(),
            family_id: Some("fixture".into()),
            content_fingerprint: None,
            added_at_unix_ms: 1,
        })
        .unwrap();
    app.library
        .set_game_settings(
            &id,
            &astra_emu_manager_core::GameSettings {
                input_mapping: Some(default_vn_preset()),
            },
        )
        .unwrap();
    let imported = app.scan_paths(&[root.path().into()]).unwrap();
    assert_eq!(imported, vec![id.clone()]);
    assert_eq!(app.library.list_games().unwrap().len(), 1);
    assert_eq!(
        app.library
            .game(&id)
            .unwrap()
            .unwrap()
            .user_title
            .as_deref(),
        Some("My game")
    );
    assert!(app
        .library
        .game_settings(&id)
        .unwrap()
        .unwrap()
        .input_mapping
        .is_some());
}

#[test]
fn scanning_existing_path_aliases_keeps_both_records_without_a_location_conflict() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let alias = root.path().join(".").join("game");
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    for (id, location) in [("original", &alias), ("canonical", &path)] {
        app.library
            .add_game(&GameRecord {
                game_id: id.into(),
                title: "game".into(),
                user_title: Some(id.into()),
                location: location.to_str().unwrap().into(),
                family_id: Some("fixture".into()),
                content_fingerprint: None,
                added_at_unix_ms: 1,
            })
            .unwrap();
    }
    app.rescan().unwrap();
    assert_eq!(app.library.list_games().unwrap().len(), 2);
    assert_eq!(
        app.library.game("original").unwrap().unwrap().location,
        alias.to_str().unwrap()
    );
    assert_eq!(
        app.library.game("canonical").unwrap().unwrap().location,
        path.to_str().unwrap()
    );
    assert!(app.candidates.contains_key("canonical"));
    assert!(app.candidates.contains_key("original"));
    std::fs::remove_file(path.join("match")).unwrap();
    app.rescan().unwrap();
    assert!(app.candidates.is_empty());
    for id in ["original", "canonical"] {
        assert_eq!(app.library.game(id).unwrap().unwrap().family_id, None);
    }
}

#[test]
fn unavailable_game_does_not_block_library_startup_or_lose_metadata() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let id = path_id(&path);
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    scan_once(&mut app, &path);
    app.library
        .set_game_user_title(&id, Some("Saved title"))
        .unwrap();
    std::fs::remove_dir_all(&path).unwrap();
    let model = app.rescan().unwrap();
    assert_eq!(model.games.len(), 1);
    assert_eq!(model.games[0].title, "Saved title");
    assert!(!model.games[0].diagnostic.is_empty());
    assert!(!app.candidates.contains_key(&id));
    assert!(model.global_diagnostic.contains("1 个游戏目录当前不可用"));
}

#[test]
fn selected_probe_is_cleared_on_no_match() {
    let root = tempfile::tempdir().unwrap();
    let game_path = game(root.path());
    let id = path_id(&game_path);
    let mut app = controller(root.path());
    app.library
        .add_game(&GameRecord {
            game_id: id.clone(),
            title: "Imported title".into(),
            user_title: Some("Custom title".into()),
            location: game_path.to_string_lossy().into_owned(),
            family_id: None,
            content_fingerprint: Some("fingerprint".into()),
            added_at_unix_ms: 7,
        })
        .unwrap();
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    scan_once(&mut app, &game_path);

    app.selected_case_id = Some(id.clone());
    assert!(app.candidates.contains_key(&id));
    assert_eq!(
        app.library.game(&id).unwrap().unwrap().family_id.as_deref(),
        Some("fixture")
    );

    std::fs::remove_file(game_path.join("match")).unwrap();
    scan_once(&mut app, &game_path);

    assert!(!app.candidates.contains_key(&id));
    assert!(!app.probe_choices.contains_key(&id));
    assert_eq!(app.library.game(&id).unwrap().unwrap().family_id, None);
    let record = app.library.game(&id).unwrap().unwrap();
    assert_eq!(record.user_title.as_deref(), Some("Custom title"));
    assert_eq!(record.content_fingerprint.as_deref(), Some("fingerprint"));
    assert_eq!(record.added_at_unix_ms, 7);
    let model = app.model().unwrap();
    assert_eq!(model.games[0].family, "probe required");
    assert_eq!(model.games[0].diagnostic, "family probe required");
    assert!(model.selected_family.is_empty());
    assert!(model.family_config_fields.is_empty());
    assert_eq!(
        app.launch(&id).unwrap_err(),
        "ASTRA_EMU_FAMILY_PROBE_REQUIRED"
    );
}

#[test]
fn ambiguous_probe_clears_old_candidate_and_rejects_old_choice_after_no_match() {
    let root = tempfile::tempdir().unwrap();
    let game_path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    scan_once(&mut app, &game_path);

    let id = path_id(&game_path);
    app.selected_case_id = Some(id.clone());
    app.registry
        .register_provider(move || provider("test-b", 8_000))
        .unwrap();
    scan_once(&mut app, &game_path);

    assert!(!app.candidates.contains_key(&id));
    assert_eq!(app.probe_choices.get(&id).unwrap().len(), 2);
    assert_eq!(app.library.game(&id).unwrap().unwrap().family_id, None);
    let model = app.model().unwrap();
    assert_eq!(model.games[0].diagnostic, "provider selection required");
    assert!(model.selected_family.is_empty());
    assert_eq!(
        app.launch(&id).unwrap_err(),
        "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_REQUIRED"
    );
    app.family_config_changed("family.plugin_id", "test-a")
        .unwrap();

    std::fs::remove_file(game_path.join("match")).unwrap();
    scan_once(&mut app, &game_path);

    assert!(!app.candidates.contains_key(&id));
    assert!(!app.probe_choices.contains_key(&id));
    assert_eq!(
        app.family_config_changed("family.plugin_id", "test-a")
            .unwrap_err(),
        "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_NOT_REQUIRED"
    );
    assert_eq!(
        app.save_family_config().unwrap_err(),
        "ASTRA_EMU_FAMILY_PROBE_REQUIRED"
    );
}

#[test]
fn selected_probe_is_recreated_after_match_returns() {
    let root = tempfile::tempdir().unwrap();
    let game_path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    scan_once(&mut app, &game_path);

    let id = path_id(&game_path);
    assert!(app.candidates.contains_key(&id));

    std::fs::remove_file(game_path.join("match")).unwrap();
    scan_once(&mut app, &game_path);
    assert!(!app.candidates.contains_key(&id));
    assert_eq!(app.library.game(&id).unwrap().unwrap().family_id, None);

    std::fs::write(game_path.join("match"), b"match").unwrap();
    scan_once(&mut app, &game_path);

    let candidate = app.candidates.get(&id).unwrap();
    assert_eq!(candidate.report.plugin_id, "test-a");
    assert_eq!(
        app.library.game(&id).unwrap().unwrap().family_id.as_deref(),
        Some("fixture")
    );
}

#[test]
fn rescan_replaces_stale_provider_choice_with_current_candidate() {
    let root = tempfile::tempdir().unwrap();
    let game_path = game(root.path());
    std::fs::rename(game_path.join("match"), game_path.join("marker-a")).unwrap();
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider_with_marker("test-a", 9_000, "marker-a"))
        .unwrap();
    scan_once(&mut app, &game_path);

    let id = path_id(&game_path);
    app.selected_case_id = Some(id.clone());
    app.registry
        .register_provider(move || provider_with_marker("test-b", 8_000, "marker-b"))
        .unwrap();
    std::fs::write(game_path.join("marker-b"), b"marker-b").unwrap();
    scan_once(&mut app, &game_path);
    assert_eq!(app.probe_choices.get(&id).unwrap().len(), 2);
    app.family_config_changed("family.plugin_id", "test-a")
        .unwrap();

    std::fs::remove_file(game_path.join("marker-a")).unwrap();
    scan_once(&mut app, &game_path);

    assert!(!app.probe_choices.contains_key(&id));
    assert_eq!(app.candidates.get(&id).unwrap().report.plugin_id, "test-b");
    assert!(!app.family_options.contains_key("family.plugin_id"));
    assert_eq!(
        app.family_config_changed("family.plugin_id", "test-a")
            .unwrap_err(),
        "ASTRA_EMU_FAMILY_PROVIDER_SELECTION_NOT_REQUIRED"
    );
    app.save_family_config().unwrap();
}
