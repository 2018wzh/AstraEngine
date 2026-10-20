use super::*;
use crate::test_native::{self, Fixture};

fn settle(app: &mut AstraEmuManagerController, ready: impl Fn(&AstraEmuManagerController) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready(app) {
        app.poll_platform().unwrap();
        assert!(
            Instant::now() < deadline,
            "public fixture operation did not finish"
        );
        std::thread::yield_now();
    }
}

fn controller(root: &Path) -> (AstraEmuManagerController, PathBuf, String) {
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("match"), b"public marker").unwrap();
    fs::write(source.join("payload.bin"), b"original public payload").unwrap();
    fs::create_dir(source.join("save")).unwrap();
    fs::write(source.join("save/slot.bin"), b"original public save").unwrap();
    let source = fs::canonicalize(source).unwrap();
    let mut app = AstraEmuManagerController::open_library(
        root.join("manager"),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    app.set_host_wake(Arc::new(|| {}));
    app.prepare_library().unwrap();
    settle(&mut app, |app| !app.startup_busy());
    app.registry
        .register_provider(|| {
            let mut fixture = Fixture::new("patch-fixture", "fixture", "fixture");
            fixture.marker = Some("match");
            fixture.config = true;
            test_native::install(0, fixture)
        })
        .unwrap();
    settle(&mut app, |app| {
        app.registry.descriptor("patch-fixture").is_some()
    });
    app.add_game_directory(&source).unwrap();
    settle(&mut app, |app| !app.startup_busy());
    let id = path_id(&source);
    let mut input = default_vn_preset();
    input.deadzone = GamepadDeadzone::High;
    app.library
        .set_game_settings(
            &id,
            &astra_emu_manager_core::GameSettings {
                emulator_id: Some("patch-fixture".into()),
                input_mapping: Some(input),
                ..Default::default()
            },
        )
        .unwrap();
    let schema = app.registry.configuration("patch-fixture").unwrap();
    app.library
        .save_family_configuration(
            "patch-fixture",
            &id,
            &schema,
            &[astra_emu_manager_core::model::ConfigEntry {
                id: "launch_mode".into(),
                value: astra_emu_manager_core::model::ConfigValue::Enum("direct".into()),
            }],
        )
        .unwrap();
    let play = app.library.start_play_session(&id, 1000).unwrap();
    app.library
        .end_play_session(&play, 2000, PlaySessionEndReason::Leave)
        .unwrap();
    app.select_case(&id).unwrap();
    (app, source, id)
}

fn recipe(path: &Path, body: &str, configuration: &str) {
    fs::write(
        path,
        format!(
            r#"return {{id="manager-public",version=1,title="公开 Manager 夹具",apply=function(ctx)
        {body}
        ctx.copy("source","match","match")
        ctx.copy("source","payload.bin","payload.bin")
        return {{plugin_id="patch-fixture",configuration={configuration}}}
    end}}"#
        ),
    )
    .unwrap();
}

#[test]
fn completion_uses_output_identity_after_selection_changes_without_copying_source_state() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, source, source_id) = controller(root.path());
    let source_record = app.library.game(&source_id).unwrap();
    let source_settings = app.library.game_settings(&source_id).unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("match"), b"other marker").unwrap();
    app.add_game_directory(&other).unwrap();
    settle(&mut app, |app| !app.startup_busy());
    let other_id = path_id(&fs::canonicalize(other).unwrap());
    let other_settings = app.library.game_settings(&other_id).unwrap();
    app.select_case(&source_id).unwrap();
    let script = root.path().join("convert.luau");
    recipe(&script, "", "{launch_mode=\"title\"}");
    let output = root.path().join("independent");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    app.select_case(&other_id).unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    let output_id = path_id(&fs::canonicalize(&output).unwrap());
    assert_eq!(app.selected_case_id.as_deref(), Some(other_id.as_str()));
    assert_eq!(app.library.list_games().unwrap().len(), 3);
    assert_eq!(app.library.game(&source_id).unwrap(), source_record);
    assert_eq!(
        app.library.game_settings(&source_id).unwrap(),
        source_settings
    );
    assert_eq!(
        app.library.game_settings(&other_id).unwrap(),
        other_settings
    );
    let schema = app.registry.configuration("patch-fixture").unwrap();
    let values = app
        .library
        .family_configuration("patch-fixture", &output_id, &schema)
        .unwrap();
    assert_eq!(
        values[0].value,
        astra_emu_manager_core::model::ConfigValue::Enum("title".into())
    );
    assert!(app
        .library
        .game_settings(&output_id)
        .unwrap()
        .unwrap()
        .input_mapping
        .is_none());
    let snapshot = app.library.library_snapshot().unwrap();
    assert_eq!(snapshot.play_stats[&source_id].total_duration_ms, 1000);
    assert_eq!(snapshot.play_stats[&output_id].total_duration_ms, 0);
    assert_eq!(
        fs::read(output.join("payload.bin")).unwrap(),
        b"original public payload"
    );
    assert!(!output.join("save").exists());
    assert_eq!(
        fs::read(source.join("save/slot.bin")).unwrap(),
        b"original public save"
    );
    assert!(
        app.patch_state.error.is_empty(),
        "{}",
        app.patch_state.error
    );
}

#[test]
fn running_patch_blocks_launch_scan_and_second_job_but_cancel_joins_and_allows_restart() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, source, source_id) = controller(root.path());
    let script = root.path().join("cancel.luau");
    recipe(&script, "ctx.create('partial'):write(0,buffer.fromstring('partial')); ctx.progress('public waiting',0,nil); while true do end", "{launch_mode=\"title\"}");
    let output = root.path().join("cancelled-output");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    settle(&mut app, |app| app.patch_state.phase == "public waiting");
    assert!(!app.model().unwrap().selection.launchable);
    assert!(app
        .start_file_patch(&script, Path::new(""), &output)
        .is_err());
    assert!(app.prepare_library().is_err());
    assert!(app.add_game_directory(&source).is_err());
    assert!(app.launch(&source_id).is_err());
    assert!(app.active.is_none());
    app.cancel_file_patch().unwrap();
    settle(&mut app, |app| app.patch_job.is_none());
    assert!(!output.exists());
    assert!(app.patch_view().can_start);
    recipe(&script, "", "{launch_mode=\"title\"}");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    assert_eq!(
        fs::read(output.join("payload.bin")).unwrap(),
        b"original public payload"
    );
    assert_eq!(
        fs::read(source.join("save/slot.bin")).unwrap(),
        b"original public save"
    );
    assert!(
        app.patch_state.error.is_empty(),
        "{}",
        app.patch_state.error
    );
}

#[test]
fn unsupported_profile_keeps_completed_output_and_does_not_bind_candidate_or_modify_source() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, source_id) = controller(root.path());
    let before = app.library.game_settings(&source_id).unwrap();
    let script = root.path().join("unsupported.luau");
    recipe(&script, "", "{unsupported_field=\"value\"}");
    let output = root.path().join("completed-but-not-imported");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    assert_eq!(
        fs::read(output.join("payload.bin")).unwrap(),
        b"original public payload"
    );
    assert!(output.join("astra-patch-result.json").is_file());
    let output_id = path_id(&fs::canonicalize(output).unwrap());
    assert!(app.patch_state.error.contains("unknown field"));
    assert!(!app.candidates.contains_key(&output_id));
    assert!(!app.probe_choices.contains_key(&output_id));
    assert!(app.library.game_settings(&output_id).unwrap().is_none());
    assert_eq!(app.library.game_settings(&source_id).unwrap(), before);
    assert_eq!(app.selected_case_id.as_deref(), Some(source_id.as_str()));
}

#[test]
fn explicit_receipt_import_reads_only_data_and_private_staging_is_never_discovered() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, source_id) = controller(root.path());
    let before = app.library.game_settings(&source_id).unwrap();
    let output = root.path().join("manual-output");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("match"), b"public manual fixture").unwrap();
    fs::write(
        output.join("must-not-run.luau"),
        b"this is deliberately not Lua",
    )
    .unwrap();
    let receipt = r#"{"schema":"astra.emu.file_patch.v1","scripts":[{"path":"must-not-run.luau","sha256":"not-executed"}],"profile":{"plugin_id":"patch-fixture","configuration":{"launch_mode":"title"}}}"#;
    fs::write(output.join("astra-patch-result.json"), receipt).unwrap();
    app.add_game_directory(&output).unwrap();
    settle(&mut app, |app| !app.startup_busy());
    let output_id = path_id(&fs::canonicalize(output).unwrap());
    let schema = app.registry.configuration("patch-fixture").unwrap();
    assert_eq!(
        app.library
            .family_configuration("patch-fixture", &output_id, &schema)
            .unwrap()[0]
            .value,
        astra_emu_manager_core::model::ConfigValue::Enum("title".into())
    );
    let staging = root.path().join(".astra-patch-staging-public");
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("match"), b"unfinished").unwrap();
    fs::write(staging.join("astra-patch-result.json"), receipt).unwrap();
    app.add_game_directory(root.path()).unwrap();
    settle(&mut app, |app| !app.startup_busy());
    assert!(!app
        .library
        .list_games()
        .unwrap()
        .iter()
        .any(|game| astra_emu_patch::is_staging_path(Path::new(&game.location))));
    assert_eq!(app.library.game_settings(&source_id).unwrap(), before);
}
