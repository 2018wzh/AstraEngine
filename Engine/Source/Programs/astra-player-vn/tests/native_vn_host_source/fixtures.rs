use super::*;

pub(super) const STORY: &str = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    text key:line.one speaker:hero #@id line.one
    text key:line.two speaker:hero #@id line.two
"#;

pub(super) fn product_package() -> Vec<u8> {
    product_package_for(STORY)
}

pub(super) fn product_package_for(story: &str) -> Vec<u8> {
    product_package_with_request(story, |_| {})
}

pub(super) fn source_for(story: &str) -> NativeVnHostCommandSource {
    let bytes = product_package_for(story);
    let package = PackageReader::open(&bytes).unwrap();
    NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap()
}

pub(super) fn advance(
    source: &mut NativeVnHostCommandSource,
) -> astra_player_core::PlayerHostCommandBatch {
    source
        .dispatch_ui_event(UiInputEventKind::Keyboard {
            logical_key: "Enter".to_string(),
            physical_key: "Enter".to_string(),
            state: UiButtonState::Pressed,
            repeat: false,
            modifiers: 0,
        })
        .unwrap()
}

pub(super) fn prepare_test_save_metadata(source: &mut NativeVnHostCommandSource, slot: &str) {
    source
        .cache_gameplay_surface(320, 180, vec![0x40; 320 * 180 * 4])
        .expect("cache gameplay surface");
    source
        .prepare_save_metadata(slot, "2000-01-01T00:00:00Z".into(), 3_723_000)
        .expect("prepare save metadata");
}

#[test]
pub(super) fn typed_ui_exit_request_stays_host_owned_and_observable() {
    let mut source = source_for(STORY);
    source.launch().expect("launch");
    assert!(!source.exit_requested());

    source
        .dispatch_ui_event(UiInputEventKind::AccessibilityAction {
            semantic_id: "root/exit".into(),
            action: "activate".into(),
            value: None,
        })
        .expect("typed exit request");

    assert!(source.exit_requested());
    assert_eq!(
        source.pending_wait().expect("dialogue wait").command_id,
        "line.one"
    );
}

#[test]
pub(super) fn secondary_pointer_opens_system_ui_without_advancing_dialogue() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    text key:line.one speaker:hero #@id line.one

story system #@id story.system
state popup #@id state.system.popup
  scene popup #@id scene.system.popup
    system_page kind:quick_panel #@id page.popup
"#;
    let mut source = source_for(story);
    source.launch().expect("launch");
    let wait_before = source
        .pending_wait()
        .expect("dialogue wait")
        .command_id
        .clone();

    source
        .dispatch_ui_event(UiInputEventKind::PointerButton {
            button: UiPointerButton::Secondary,
            state: UiButtonState::Pressed,
            position: astra_ui_core::UiPoint { x: 12.0, y: 12.0 },
        })
        .expect("secondary pointer dispatch");

    assert_eq!(
        source.pending_wait().expect("system page wait").command_id,
        "page.popup"
    );
    assert_ne!(source.pending_wait().unwrap().command_id, wait_before);
    assert!(source
        .ui_semantics()
        .expect("system semantics")
        .nodes
        .iter()
        .any(|node| node.id == "root/back"));

    source
        .dispatch_ui_event(UiInputEventKind::Keyboard {
            logical_key: "Escape".to_string(),
            physical_key: "Escape".to_string(),
            state: UiButtonState::Pressed,
            repeat: false,
            modifiers: 0,
        })
        .expect("Escape returns from system UI");
    assert_eq!(
        source
            .pending_wait()
            .expect("restored dialogue wait")
            .command_id,
        wait_before
    );

    source.release_resources().expect("release");
    source.shutdown().expect("shutdown");
}

#[test]
pub(super) fn secondary_pointer_round_trips_the_same_choice_occurrence() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    choice key:line.one #@id choice.prompt
      option key:line.one target:first #@id option.first
      option key:line.one target:second #@id option.second

state first #@id state.first
  scene first #@id scene.first
    input_wait #@id wait.first

state second #@id state.second
  scene second #@id scene.second
    input_wait #@id wait.second

story system #@id story.system
state popup #@id state.system.popup
  scene popup #@id scene.system.popup
    system_page kind:quick_panel #@id page.popup
"#;
    let mut source = source_for(story);
    source.launch().expect("launch");
    let choice_wait = source.pending_wait().expect("choice wait").clone();
    let choice = source
        .last_step_evidence()
        .expect("choice evidence")
        .pending_choice_ids
        .clone();

    source
        .dispatch_ui_event(UiInputEventKind::PointerButton {
            button: UiPointerButton::Secondary,
            state: UiButtonState::Pressed,
            position: astra_ui_core::UiPoint { x: 12.0, y: 12.0 },
        })
        .expect("secondary pointer dispatch");
    assert_eq!(
        source.pending_wait().expect("system page wait").command_id,
        "page.popup"
    );

    source
        .dispatch_ui_event(UiInputEventKind::Keyboard {
            logical_key: "Escape".to_string(),
            physical_key: "Escape".to_string(),
            state: UiButtonState::Pressed,
            repeat: false,
            modifiers: 0,
        })
        .expect("Escape returns from system UI");
    assert_eq!(source.pending_wait(), Some(&choice_wait));
    assert_eq!(
        source
            .last_step_evidence()
            .expect("restored choice evidence")
            .pending_choice_ids,
        choice
    );
}

#[test]
pub(super) fn product_observation_reports_system_ui_without_exposing_content() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    text key:line.one speaker:hero #@id line.one

story system #@id story.system
state popup #@id state.system.popup
  scene popup #@id scene.system.popup
    system_page kind:quick_panel #@id page.popup
"#;
    let mut source = source_for(story);
    source.launch().expect("launch");

    let initial = source
        .product_observation_evidence()
        .expect("initial product observation");
    assert_eq!(initial.schema, "astra.player_vn_product_observation.v3");
    assert_eq!(initial.ui_profile, "classic");
    assert_eq!(initial.locale, "en");
    assert_eq!(initial.active_system_page, None);
    assert_eq!(initial.focused_semantic_id.as_deref(), Some("root/advance"));
    assert!(initial.skip_allowed);
    assert_eq!(initial.backlog_count, 1);
    assert_eq!(initial.occupied_save_slot_count, 0);

    source
        .dispatch_ui_event(UiInputEventKind::PointerButton {
            button: UiPointerButton::Secondary,
            state: UiButtonState::Pressed,
            position: astra_ui_core::UiPoint { x: 12.0, y: 12.0 },
        })
        .expect("secondary pointer dispatch");
    let system = source
        .product_observation_evidence()
        .expect("system product observation");
    assert_eq!(
        system.active_system_page,
        Some(astra_vn_core::SystemPageKind::QuickPanel)
    );
    assert_eq!(system.focused_semantic_id.as_deref(), Some("root/back"));
    assert_eq!(system.backlog_count, 1);

    source.release_resources().expect("release");
    source.shutdown().expect("shutdown");
}

#[test]
pub(super) fn secondary_pointer_opens_system_ui_while_story_wait_has_no_ui_surface() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    wait fence:opening.audio.end #@id wait.opening.audio
    text key:line.one speaker:hero #@id line.one

story system #@id story.system
state popup #@id state.system.popup
  scene popup #@id scene.system.popup
    system_page kind:quick_panel #@id page.popup
"#;
    let mut source = source_for(story);
    source.launch().expect("launch");
    assert_eq!(
        source.pending_wait().expect("opening wait").command_id,
        "wait.opening.audio"
    );

    source
        .dispatch_ui_event(UiInputEventKind::PointerButton {
            button: UiPointerButton::Secondary,
            state: UiButtonState::Pressed,
            position: astra_ui_core::UiPoint { x: 12.0, y: 12.0 },
        })
        .expect("secondary pointer dispatch");

    assert_eq!(
        source.pending_wait().expect("system page wait").command_id,
        "page.popup"
    );
    source.release_resources().expect("release");
    source.shutdown().expect("shutdown");
}

#[test]
pub(super) fn primary_pointer_advances_input_wait_without_fabricating_message_ui() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    input_wait #@id wait.input
    text key:line.one speaker:hero #@id line.one
"#;
    let mut source = source_for(story);
    source.launch().expect("launch");
    assert_eq!(
        source.pending_wait().expect("input wait").command_id,
        "wait.input"
    );
    assert!(source.ui_semantics().is_none());

    source
        .dispatch_ui_event(UiInputEventKind::PointerButton {
            button: UiPointerButton::Primary,
            state: UiButtonState::Pressed,
            position: astra_ui_core::UiPoint { x: 12.0, y: 12.0 },
        })
        .expect("primary pointer dispatch");

    assert_eq!(
        source.pending_wait().expect("dialogue wait").command_id,
        "line.one"
    );
    assert!(source
        .ui_semantics()
        .expect("message semantics")
        .nodes
        .iter()
        .any(|node| node.id == "root/advance"));
    source.release_resources().expect("release");
    source.shutdown().expect("shutdown");
}

#[test]
pub(super) fn controller_on_open_focuses_a_stable_semantic_target() {
    let mut source = source_for(STORY);
    source.launch().expect("launch");
    source
        .dispatch_ui_event(UiInputEventKind::FixedTime {
            time_ns: 16_666_667,
        })
        .expect("second UI frame");
    let semantics = source.ui_semantics().expect("semantic snapshot");
    assert!(semantics
        .nodes
        .iter()
        .any(|node| node.id == "root/advance" && node.focused));
    source.release_resources().expect("release");
    source.shutdown().expect("shutdown");
}

#[test]
pub(super) fn product_source_records_bounded_ui_performance_samples() {
    let mut source = source_for(STORY);
    source.launch().unwrap();
    for frame in 0..30 {
        source
            .dispatch_ui_event(UiInputEventKind::FixedTime {
                time_ns: frame * 16_666_667,
            })
            .unwrap();
    }
    let report = source.ui_performance_report();
    assert_eq!(report.schema, "astra.ui_performance_report.v1");
    assert_eq!(report.sample_count, 31);
    assert!(report.peak_draw_calls <= 128);
    assert!(report.peak_vertices <= 250_000);
    assert!(report.peak_active_texture_bytes <= 64 * 1024 * 1024);
}

#[test]
pub(super) fn stable_pointer_motion_reuses_ui_until_the_semantic_hit_region_changes() {
    let mut source = source_for(STORY);
    source.launch().expect("launch");
    let semantics = source.ui_semantics().expect("semantic snapshot").clone();
    let exit = semantics
        .nodes
        .iter()
        .find(|node| node.id == "root/exit")
        .expect("exit semantics");
    let inside_exit = UiPoint {
        x: (exit.bounds_points.min.x + exit.bounds_points.max.x) * 0.5,
        y: (exit.bounds_points.min.y + exit.bounds_points.max.y) * 0.5,
    };
    let outside_exit = UiPoint {
        x: if exit.bounds_points.min.x >= 2.0 {
            exit.bounds_points.min.x - 2.0
        } else {
            exit.bounds_points.max.x + 2.0
        },
        y: inside_exit.y,
    };
    let outside_exit_neighbour = UiPoint {
        x: outside_exit.x + 0.25,
        y: outside_exit.y,
    };

    source
        .dispatch_ui_event(UiInputEventKind::PointerMove {
            position: outside_exit,
        })
        .expect("first pointer frame");
    source
        .take_last_ui_performance_sample()
        .expect("first performance sample");
    source
        .dispatch_ui_event(UiInputEventKind::PointerMove {
            position: outside_exit_neighbour,
        })
        .expect("stable pointer frame");
    let reused = source
        .take_last_ui_performance_sample()
        .expect("reused performance sample");
    assert_eq!(reused.update_layout_ns, 0);
    assert_eq!(reused.paint_conversion_ns, 0);
    assert_eq!(reused.texture_update_bytes, 0);

    source
        .dispatch_ui_event(UiInputEventKind::PointerMove {
            position: inside_exit,
        })
        .expect("changed hit region");
    let invalidated = source
        .take_last_ui_performance_sample()
        .expect("invalidated performance sample");
    assert_ne!(invalidated.update_layout_ns, 0);

    source.release_resources().expect("release");
    source.shutdown().expect("shutdown");
}
