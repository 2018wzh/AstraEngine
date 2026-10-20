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
    let mut app =
        AstraEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new())
            .unwrap();
    app.prepare_library().unwrap();
    settle_background(&mut app);
    app
}

fn game(root: &Path) -> PathBuf {
    let path = root.join("game");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("match"), b"match").unwrap();
    std::fs::canonicalize(path).unwrap()
}

pub(super) fn settle_background(app: &mut AstraEmuManagerController) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.startup_busy() {
        app.poll_platform().unwrap();
        assert!(
            Instant::now() < deadline,
            "background operation did not complete"
        );
        std::thread::yield_now();
    }
}

fn settle_provider(app: &mut AstraEmuManagerController) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.cores_ready() {
        app.poll_platform().unwrap();
        assert!(
            Instant::now() < deadline,
            "provider metadata did not arrive"
        );
        std::thread::yield_now();
    }
}

fn scan_paths(app: &mut AstraEmuManagerController, paths: &[PathBuf]) -> Vec<String> {
    settle_provider(app);
    app.begin_scan(probe_worker::ScanKind::Import, paths.to_vec())
        .unwrap();
    settle_background(app);
    app.library
        .list_games()
        .unwrap()
        .into_iter()
        .map(|game| game.game_id)
        .collect()
}

fn scan_once(app: &mut AstraEmuManagerController, path: &Path) {
    scan_paths(app, &[path.to_owned()]);
}

fn import(app: &mut AstraEmuManagerController, path: &Path) -> ManagerViewModel {
    settle_provider(app);
    app.add_game_directory(path).unwrap();
    settle_background(app);
    app.model().unwrap()
}

#[test]
fn core_loading_preserves_cached_library_bindings_until_an_explicit_scan() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("manager");
    let games = root.path().join("games");
    let path = game(&games);
    let discovered = games.join("discovered");
    fs::create_dir(&discovered).unwrap();
    fs::write(discovered.join("match"), b"match").unwrap();
    let id = path_id(&path);
    {
        let mut saved = AstraEmuManagerController::open_library(
            data.clone(),
            FrameMailbox::new(),
            vec![games.clone()],
        )
        .unwrap();
        saved
            .library
            .add_game(&GameRecord {
                game_id: id.clone(),
                title: "Cached game".into(),
                user_title: Some("Saved title".into()),
                location: path.to_str().unwrap().into(),
                family_id: Some("saved-family".into()),
                content_fingerprint: Some("saved-fingerprint".into()),
                added_at_unix_ms: 7,
            })
            .unwrap();
        saved
            .library
            .set_game_settings(
                &id,
                &astra_emu_manager_core::GameSettings {
                    emulator_id: Some("test-a".into()),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let mut app =
        AstraEmuManagerController::open_library(data.clone(), FrameMailbox::new(), vec![games])
            .unwrap();
    let initial = app.model().unwrap();
    assert_eq!(initial.library.games.len(), 1);
    assert_eq!(initial.library.games[0].title, "Saved title");
    assert!(!initial.library.busy);
    assert!(!initial.library.cores_ready);
    assert_eq!(initial.library.prepare_state, "unprepared");
    assert_eq!(initial.library.games[0].problem, "需要先准备资料库");
    assert!(app.poll_platform().unwrap().is_none());
    assert!(!data.join("cores").exists());
    assert_eq!(app.search("Saved").unwrap().library.games.len(), 1);
    app.search("").unwrap();

    let pending = app.prepare_library().unwrap();
    assert!(pending.library.busy);
    assert_eq!(pending.library.prepare_state, "loading-cores");
    assert_eq!(pending.library.games.len(), 1);
    assert_eq!(pending.library.games[0].problem, "正在准备资料库");
    assert_eq!(app.prepare_library().unwrap_err(), "ASTRA_EMU_PREPARE_BUSY");
    settle_background(&mut app);
    assert_eq!(
        app.game(&id).unwrap().family_id.as_deref(),
        Some("saved-family")
    );
    assert_eq!(app.library.list_games().unwrap().len(), 1);
    assert!(app.candidates.is_empty());
    assert!(app.probe_choices.is_empty());
    assert!(app.cores_started());

    let calls = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = calls.clone();
    app.registry
        .register_provider(move || {
            let mut fixture = Fixture::new("test-a", "fixture", "fixture");
            fixture.marker = Some("match");
            fixture.calls = sink;
            test_native::install(0, fixture)
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.registry.descriptor("test-a").is_none() {
        app.poll_platform().unwrap();
        assert!(
            Instant::now() < deadline,
            "provider metadata did not arrive"
        );
        std::thread::yield_now();
    }
    let loaded = app.model().unwrap();
    assert!(loaded.library.cores_ready);
    assert!(!loaded.library.busy);
    assert_eq!(loaded.library.prepare_state, "ready");
    assert_eq!(loaded.library.games.len(), 1);
    assert!(!loaded.selection.launchable);
    assert_eq!(
        app.game(&id).unwrap().family_id.as_deref(),
        Some("saved-family")
    );
    assert_eq!(
        app.library
            .game_settings(&id)
            .unwrap()
            .unwrap()
            .emulator_id
            .as_deref(),
        Some("test-a")
    );
    assert!(app.candidates.is_empty());
    assert!(!calls.lock().iter().any(|call| call.name == "probe"));
    assert_eq!(loaded.library.games[0].problem, "未识别到可用的模拟器");

    app.prepare_library().unwrap();
    settle_background(&mut app);
    let scanned = app.model().unwrap();
    assert_eq!(scanned.library.games.len(), 2);
    assert!(scanned.selection.launchable);
    assert_eq!(app.game(&id).unwrap().family_id.as_deref(), Some("fixture"));
    assert_eq!(
        app.game(&id).unwrap().content_fingerprint.as_deref(),
        Some("saved-fingerprint")
    );
    assert_eq!(
        app.game(&id).unwrap().user_title.as_deref(),
        Some("Saved title")
    );
    assert_eq!(
        app.library
            .game_settings(&id)
            .unwrap()
            .unwrap()
            .emulator_id
            .as_deref(),
        Some("test-a")
    );
    assert!(calls.lock().iter().any(|call| call.name == "probe"));
}

#[test]
fn import_selects_the_imported_game() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    let model = import(&mut app, &path);
    assert_eq!(model.library.selected_id, path_id(&path));
}

#[test]
fn unsupported_import_preserves_the_existing_game_and_selection() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    import(&mut app, &path);
    let unsupported = root.path().join("unsupported");
    std::fs::create_dir(&unsupported).unwrap();
    let model = import(&mut app, &unsupported);
    assert_eq!(model.library.games.len(), 1);
    assert_eq!(model.library.selected_id, path_id(&path));
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
    let imported = scan_paths(&mut app, &[root.path().into(), assets]);
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
                ..Default::default()
            },
        )
        .unwrap();
    let imported = scan_paths(&mut app, &[root.path().into()]);
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
    assert!(
        app.library
            .game_settings(&id)
            .unwrap()
            .unwrap()
            .input_mapping
            .is_some()
    );
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
    settle_provider(&mut app);
    app.prepare_library().unwrap();
    settle_background(&mut app);
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
    app.prepare_library().unwrap();
    settle_background(&mut app);
    assert!(app.candidates.is_empty());
    for id in ["original", "canonical"] {
        assert_eq!(
            app.library.game(id).unwrap().unwrap().family_id.as_deref(),
            Some("fixture")
        );
    }
}

#[test]
fn manual_full_scan_preserves_unavailable_game_metadata() {
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
    app.prepare_library().unwrap();
    settle_background(&mut app);
    let model = app.model().unwrap();
    assert_eq!(model.library.games.len(), 1);
    assert_eq!(model.library.games[0].title, "Saved title");
    assert!(!model.library.games[0].problem.is_empty());
    assert!(!app.candidates.contains_key(&id));
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
    assert_eq!(
        app.library.game(&id).unwrap().unwrap().family_id.as_deref(),
        Some("fixture")
    );
    let record = app.library.game(&id).unwrap().unwrap();
    assert_eq!(record.user_title.as_deref(), Some("Custom title"));
    assert_eq!(record.content_fingerprint.as_deref(), Some("fingerprint"));
    assert_eq!(record.added_at_unix_ms, 7);
    let model = app.model().unwrap();
    assert!(model.selection.family_fields.is_empty());
    assert!(!model.selection.launchable);
    assert_eq!(
        app.library
            .game_settings(&id)
            .unwrap()
            .unwrap()
            .emulator_id
            .as_deref(),
        Some("test-a")
    );
    assert_eq!(
        app.launch(&id).unwrap_err(),
        "ASTRA_EMU_FAMILY_PROBE_REQUIRED"
    );
}

#[test]
fn selected_emulator_survives_additional_same_engine_matches_and_reopening() {
    let root = tempfile::tempdir().unwrap();
    let game_path = game(root.path());
    let id = path_id(&game_path);
    {
        let mut app = controller(root.path());
        app.registry
            .register_provider(move || provider("test-a", 9_000))
            .unwrap();
        scan_once(&mut app, &game_path);
        app.selected_case_id = Some(id.clone());
        app.registry
            .register_provider(move || provider("test-b", 8_000))
            .unwrap();
        scan_once(&mut app, &game_path);
        assert_eq!(app.candidates[&id].report.plugin_id, "test-a");
        assert_eq!(app.probe_choices[&id].len(), 2);
        app.set_game_emulator(&id, "test-b").unwrap();
        assert_eq!(app.candidates[&id].report.plugin_id, "test-b");
        app.set_game_input_override(&id, false).unwrap();
        assert_eq!(
            app.library
                .game_settings(&id)
                .unwrap()
                .unwrap()
                .emulator_id
                .as_deref(),
            Some("test-b")
        );
    }
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    app.registry
        .register_provider(move || provider("test-b", 8_000))
        .unwrap();
    scan_once(&mut app, &game_path);
    assert_eq!(app.candidates[&id].report.plugin_id, "test-b");
    app.selected_case_id = Some(id);
    assert!(app.model().unwrap().selection.launchable);
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
    assert_eq!(
        app.library.game(&id).unwrap().unwrap().family_id.as_deref(),
        Some("fixture")
    );

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
fn missing_saved_emulator_requires_an_explicit_replacement() {
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

    std::fs::remove_file(game_path.join("marker-a")).unwrap();
    scan_once(&mut app, &game_path);

    assert!(!app.candidates.contains_key(&id));
    assert_eq!(app.probe_choices[&id][0].report.plugin_id, "test-b");
    assert!(!app.model().unwrap().selection.launchable);
    assert_eq!(
        app.library
            .game_settings(&id)
            .unwrap()
            .unwrap()
            .emulator_id
            .as_deref(),
        Some("test-a")
    );
    assert!(app.set_game_emulator(&id, "test-a").is_err());
    app.set_game_emulator(&id, "test-b").unwrap();
    assert_eq!(app.candidates[&id].report.plugin_id, "test-b");
    assert_eq!(
        app.library
            .game_settings(&id)
            .unwrap()
            .unwrap()
            .emulator_id
            .as_deref(),
        Some("test-b")
    );
}

#[test]
fn import_returns_while_owner_is_busy_and_publishes_only_after_native_work_finishes() {
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(|| provider("test-a", 9_000))
        .unwrap();
    settle_provider(&mut app);
    let (sender, gate) = std::sync::mpsc::channel();
    let mut release = Release(Some(sender));
    let (entered, receiver) = std::sync::mpsc::channel();
    app.registry
        .register_provider(move || {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            provider_with_marker("test-b", 8_000, "absent-marker")
        })
        .unwrap();
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let pending = app.add_game_directory(&path).unwrap();
    assert!(pending.library.games.is_empty());
    assert!(app.startup_busy());
    assert!(app.poll_platform().unwrap().is_none());
    release.0.take().unwrap().send(()).unwrap();
    settle_background(&mut app);
    let model = app.model().unwrap();
    assert_eq!(model.library.games.len(), 1);
    assert_eq!(model.library.selected_id, path_id(&path));
    assert!(app.candidates.contains_key(&path_id(&path)));
}

#[test]
fn manual_core_failure_requires_restart_and_shutdown_still_joins_the_owner() {
    let root = tempfile::tempdir().unwrap();
    let mut app = AstraEmuManagerController::open_library(
        root.path().into(),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    let initial = app.model().unwrap();
    assert!(!initial.library.busy);
    assert!(!initial.library.cores_ready);
    assert_eq!(initial.library.prepare_state, "unprepared");
    // The core installation is untouched until the explicit action.
    fs::write(root.path().join("cores"), b"not a directory").unwrap();
    assert!(app.poll_platform().unwrap().is_none());
    assert!(!app.cores_started());
    let pending = app.prepare_library().unwrap();
    assert!(pending.library.busy);
    assert_eq!(pending.library.prepare_state, "loading-cores");
    assert!(!pending.library.can_cancel_prepare);
    assert_eq!(
        app.cancel_prepare().unwrap_err(),
        "ASTRA_EMU_PREPARE_NOT_CANCELLABLE"
    );
    settle_background(&mut app);
    assert!(!app.startup_busy());
    assert!(
        app.startup_status()
            .contains("ASTRA_EMU_CORES_DIRECTORY_CREATE")
    );
    assert!(app.diagnostic.contains("ASTRA_EMU_CORES_DIRECTORY_CREATE"));
    assert!(app.startup_status().contains("重启"));
    assert!(!app.cores_ready());
    let failed = app.model().unwrap();
    assert_eq!(failed.library.prepare_state, "failed");
    assert_eq!(
        app.prepare_library().unwrap_err(),
        "ASTRA_EMU_PREPARE_RESTART_REQUIRED"
    );
    assert_eq!(
        app.add_game_directory(root.path()).unwrap_err(),
        "ASTRA_EMU_PREPARE_RESTART_REQUIRED"
    );
    app.registry.shutdown().unwrap();
    app.registry.shutdown().unwrap();
}

#[test]
fn failed_idle_owner_wakes_the_host_and_preserves_its_join_error() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    let (sender, receiver) = crossbeam_channel::bounded(1);
    app.set_host_wake(std::sync::Arc::new(move || {
        let _ = sender.try_send(());
    }));
    app.registry
        .register_provider(|| panic!("intentional owner failure"))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(app.startup_state, background::StartupState::Failed(_)) {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        app.poll_platform().unwrap();
    }
    assert!(app.startup_status().contains("ASTRA_EMU_OWNER_PANICKED"));
    assert!(
        app.registry
            .shutdown()
            .unwrap_err()
            .contains("ASTRA_EMU_OWNER_PANICKED")
    );
}

fn root_with_games(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let library_root = root.join("library");
    for name in ["g1", "g2"] {
        let game = library_root.join(name);
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("match"), b"match").unwrap();
    }
    let library_root = fs::canonicalize(library_root).unwrap();
    (
        library_root.clone(),
        library_root.join("g1"),
        library_root.join("g2"),
    )
}

fn visible_games(app: &AstraEmuManagerController) -> Vec<String> {
    app.library
        .library_snapshot()
        .unwrap()
        .games
        .into_iter()
        .map(|game| game.game_id)
        .collect()
}

fn prepared(app: &mut AstraEmuManagerController) -> ManagerViewModel {
    app.prepare_library().unwrap();
    settle_background(app);
    app.model().unwrap()
}

#[test]
fn removed_games_stay_hidden_across_scans_until_undone() {
    let root = tempfile::tempdir().unwrap();
    let (library_root, g1, g2) = root_with_games(root.path());
    let (g1, g2) = (path_id(&g1), path_id(&g2));
    let mut app = controller(&root.path().join("manager"));
    app.library
        .add_library_root(library_root.to_str().unwrap())
        .unwrap();
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    settle_provider(&mut app);

    let first = prepared(&mut app);
    assert_eq!(first.snackbar.text, "发现 2 部新作品");
    let mut ids = visible_games(&app);
    ids.sort();
    let mut expected = vec![g1.clone(), g2.clone()];
    expected.sort();
    assert_eq!(ids, expected);
    let session = app.library.start_play_session(&g1, 1_000).unwrap();
    app.library
        .end_play_session(&session, 61_000, PlaySessionEndReason::Leave)
        .unwrap();
    let stats = app.library.library_snapshot().unwrap().play_stats[&g1].clone();

    app.selected_case_id = Some(g1.clone());
    let removed = app.remove_from_library(&g1).unwrap();
    assert_eq!(visible_games(&app), vec![g2.clone()]);
    assert_eq!(removed.library.selected_id, "");
    assert_eq!(removed.snackbar.action, "撤销");
    assert!(removed.snackbar.text.contains("已从资料库移除"));
    assert_eq!(removed.library.excluded.len(), 1);
    assert_eq!(removed.library.excluded[0].id, g1);

    let rescanned = prepared(&mut app);
    assert_eq!(visible_games(&app), vec![g2.clone()]);
    assert_eq!(rescanned.snackbar.text, "资料库已是最新");

    app.undo_remove().unwrap();
    let mut ids = visible_games(&app);
    ids.sort();
    assert_eq!(ids, expected);
    assert_eq!(
        app.library.library_snapshot().unwrap().play_stats[&g1],
        stats
    );
    assert!(app.model().unwrap().library.excluded.is_empty());
    assert_eq!(app.undo_remove().unwrap_err(), "ASTRA_EMU_UNDO_UNAVAILABLE");

    app.remove_from_library(&g2).unwrap();
    match app.snackbar_action().unwrap() {
        astra_emu_manager::SnackbarOutcome::Model(model) => {
            assert!(model.library.games.iter().any(|game| game.id == g2));
        }
        astra_emu_manager::SnackbarOutcome::Restart => panic!("undo must not restart"),
    }
    app.remove_from_library(&g2).unwrap();
    app.restore_game(&g2).unwrap();
    assert!(visible_games(&app).contains(&g2));
}

#[test]
fn added_directory_is_persisted_as_a_library_root() {
    let root = tempfile::tempdir().unwrap();
    let (library_root, g1, _) = root_with_games(root.path());
    let mut app = controller(&root.path().join("manager"));
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    settle_provider(&mut app);
    app.add_game_directory(&library_root.join("g1")).unwrap();
    settle_background(&mut app);
    let model = app.model().unwrap();
    assert_eq!(model.library.roots, vec![g1.to_str().unwrap().to_owned()]);
    assert_eq!(model.library.selected_id, path_id(&g1));
    assert_eq!(app.scan_roots().unwrap(), vec![g1]);
}

#[test]
fn prepare_loads_cores_then_scans_every_root_automatically() {
    let root = tempfile::tempdir().unwrap();
    let (library_root, g1, g2) = root_with_games(root.path());
    let mut app = AstraEmuManagerController::open_library(
        root.path().join("manager"),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    app.library
        .add_library_root(library_root.to_str().unwrap())
        .unwrap();
    let (sender, wakes) = crossbeam_channel::unbounded();
    app.set_host_wake(std::sync::Arc::new(move || {
        let _ = sender.send(());
    }));
    while wakes.try_recv().is_ok() {}
    let pending = app.prepare_library().unwrap();
    assert_eq!(pending.library.prepare_state, "loading-cores");
    assert!(matches!(app.startup_state, background::StartupState::Loading));
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    // Core initialization and the provider registration each wake the host once.
    for _ in 0..2 {
        wakes.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    app.poll_platform().unwrap();
    assert!(matches!(
        app.startup_state,
        background::StartupState::Scanning(probe_worker::ScanKind::Full)
    ));
    let scanning = app.model().unwrap();
    assert_eq!(scanning.library.prepare_state, "scanning");
    assert!(scanning.library.can_cancel_prepare);
    settle_background(&mut app);
    assert!(matches!(app.startup_state, background::StartupState::Ready));
    let ready = app.model().unwrap();
    assert_eq!(ready.library.prepare_state, "ready");
    assert_eq!(ready.library.prepare_label, "刷新资料库");
    assert_eq!(ready.snackbar.text, "发现 2 部新作品");
    let mut ids = visible_games(&app);
    ids.sort();
    let mut expected = vec![path_id(&g1), path_id(&g2)];
    expected.sort();
    assert_eq!(ids, expected);
}

#[test]
fn cancelling_a_scan_restores_ready_without_touching_the_library() {
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let (library_root, g1, _) = root_with_games(root.path());
    let mut app = controller(&root.path().join("manager"));
    app.registry
        .register_provider(|| provider("test-a", 9_000))
        .unwrap();
    scan_once(&mut app, &g1);
    let before = visible_games(&app);
    app.library
        .add_library_root(library_root.to_str().unwrap())
        .unwrap();
    let (sender, gate) = std::sync::mpsc::channel();
    let mut release = Release(Some(sender));
    let (entered, receiver) = std::sync::mpsc::channel();
    app.registry
        .register_provider(move || {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            provider_with_marker("test-b", 8_000, "absent-marker")
        })
        .unwrap();
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    app.prepare_library().unwrap();
    assert!(matches!(
        app.startup_state,
        background::StartupState::Scanning(probe_worker::ScanKind::Full)
    ));
    app.cancel_prepare().unwrap();
    release.0.take().unwrap().send(()).unwrap();
    settle_background(&mut app);
    assert!(matches!(app.startup_state, background::StartupState::Ready));
    assert_eq!(visible_games(&app), before);
    let model = app.model().unwrap();
    assert_eq!(model.snackbar.text, "已取消扫描");
    assert_eq!(model.snackbar.action, "");
    assert!(app.diagnostic.is_empty());
    assert_eq!(
        app.cancel_prepare().unwrap_err(),
        "ASTRA_EMU_PREPARE_NOT_CANCELLABLE"
    );
}

fn fake_session(app: &mut AstraEmuManagerController, game_id: &str) {
    let epoch = astra_emu_manager_core::model::allocate_session_epoch().unwrap();
    app.active = Some(owner::ActiveSessionView::opening(epoch, game_id.into()));
}

#[test]
fn closing_the_window_leaves_a_running_game_and_keeps_the_manager_open() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    assert!(matches!(
        astra_emu_manager::close_requested(&mut app),
        astra_emu_manager::CloseDecision::HideWindow
    ));
    fake_session(&mut app, "game");
    assert!(ManagerController::is_game_active(&app));
    match astra_emu_manager::close_requested(&mut app) {
        astra_emu_manager::CloseDecision::KeepWindow(result) => {
            result.unwrap();
        }
        astra_emu_manager::CloseDecision::HideWindow => panic!("running game must keep window"),
    }
    assert!(!ManagerController::is_game_active(&app));
    assert_eq!(
        app.active.as_ref().unwrap().closing,
        Some(PlaySessionEndReason::Leave)
    );
}

#[test]
fn a_full_input_queue_drops_input_without_ending_the_session() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    fake_session(&mut app, "game");
    let mut pressed = true;
    let error = loop {
        match app.game_input("a", pressed, 1.0) {
            Ok(()) => pressed = !pressed,
            Err(error) => break error,
        }
    };
    assert_eq!(error, "ASTRA_EMU_INPUT_QUEUE_FULL");
    assert_eq!(
        app.pending_events.len(),
        astra_emu_manager_core::model::MAX_INPUT_EVENTS
    );
    assert!(ManagerController::is_game_active(&app));
    assert!(app.active.as_ref().unwrap().closing.is_none());
}

#[test]
fn key_capture_binds_named_keys_and_waits_on_unmappable_ones() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let mut app = controller(root.path());
    app.registry
        .register_provider(move || provider("test-a", 9_000))
        .unwrap();
    scan_once(&mut app, &path);
    let id = path_id(&path);

    let waiting = app.begin_key_capture("south", None).unwrap();
    assert_eq!(waiting.settings.input.capture_button, "south");
    let hint = app.capture_key(KeyCode::Digit1).unwrap();
    assert_eq!(hint.settings.input.capture_hint, input::UNMAPPABLE_KEY_HINT);
    assert!(app.key_capture_active());
    app.capture_key(KeyCode::Escape).unwrap();
    assert!(!app.key_capture_active());
    assert_eq!(app.input_mapping.gamepad[&GamepadInput::South], "escape");

    app.begin_key_capture("west", Some(&id)).unwrap();
    let bound = app.capture_key(KeyCode::F5).unwrap();
    assert_eq!(bound.settings.input.capture_button, "");
    let game_mapping = app
        .library
        .game_settings(&id)
        .unwrap()
        .unwrap()
        .input_mapping
        .unwrap();
    assert_eq!(game_mapping.gamepad[&GamepadInput::West], "f5");
    assert!(!app.input_mapping.gamepad.contains_key(&GamepadInput::West));

    app.begin_key_capture("north", None).unwrap();
    app.cancel_key_capture().unwrap();
    assert_eq!(
        app.capture_key(KeyCode::A).unwrap_err(),
        "ASTRA_EMU_KEY_CAPTURE_INACTIVE"
    );
    assert!(app.begin_key_capture("bogus", None).is_err());
}
