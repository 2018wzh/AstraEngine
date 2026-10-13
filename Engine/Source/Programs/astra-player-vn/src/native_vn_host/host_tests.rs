use super::stage::{
    default_runtime_launch_state, state_prefetch_successors, system_action_gameplay_entry_states,
};
use super::ui_layout::{
    frame_localization_subset, ordered_ui_font_families, parse_ui_text_alignment,
    retained_pointer_activation_target, reusable_ui_draw_commands, save_slots_for_policy,
};
use super::{
    NativeVnDecodedCacheBudget, ReadingMode, SaveCompletionPolicy, SystemPageKind,
    SystemUiProfilePolicy, UiBlueprintModalFrameModel, UiTextAlignment, UiValue,
    DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES,
};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn reusable_ui_frame_does_not_replay_resource_lifecycle() {
    let draw = vec![
        astra_media_core::SceneCommand::ReleaseResource {
            resource_id: "glyph.cached".into(),
        },
        astra_media_core::SceneCommand::rect("ui.cached.panel", 0, 0, 8, 8, [1, 2, 3, 255]),
    ];

    assert_eq!(
        reusable_ui_draw_commands(&draw),
        vec![astra_media_core::SceneCommand::rect(
            "ui.cached.panel",
            0,
            0,
            8,
            8,
            [1, 2, 3, 255],
        )],
    );
}

#[test]
fn image_prefetch_control_flow_prioritizes_branch_targets_over_linear_fallthrough() {
    let state = astra_vn_core::State {
        id: "root".into(),
        name: "root".into(),
        story_id: "story".into(),
        scenes: vec![astra_vn_core::Scene {
            id: "scene".into(),
            name: "scene".into(),
            commands: vec![astra_vn_core::CompiledCommand::Branch {
                id: "branch".into(),
                scope: "session".into(),
                key: "route".into(),
                op: astra_vn_core::BranchOp::Eq,
                value: 1,
                then_target: "then".into(),
                else_target: "else".into(),
            }],
        }],
    };
    let authored = BTreeMap::from([("root".into(), "linear".into())]);
    assert_eq!(
        state_prefetch_successors(&state, &authored),
        vec!["then".to_string(), "else".to_string()]
    );
}

#[test]
fn image_prefetch_control_flow_retains_call_return_fallthrough() {
    let state = astra_vn_core::State {
        id: "root".into(),
        name: "root".into(),
        story_id: "story".into(),
        scenes: vec![astra_vn_core::Scene {
            id: "scene".into(),
            name: "scene".into(),
            commands: vec![astra_vn_core::CompiledCommand::Call {
                id: "call".into(),
                target: "callee".into(),
            }],
        }],
    };
    let authored = BTreeMap::from([("root".into(), "linear".into())]);
    assert_eq!(
        state_prefetch_successors(&state, &authored),
        vec!["callee".to_string(), "linear".to_string()]
    );
}

#[test]
fn image_prewarm_resolves_system_action_target_aliases_before_ranking_gameplay_entries() {
    let states = BTreeMap::from([
        (
            "state.title".to_string(),
            astra_vn_core::State {
                id: "state.title".into(),
                name: "title".into(),
                story_id: "system.title".into(),
                scenes: Vec::new(),
            },
        ),
        (
            "state.opening".to_string(),
            astra_vn_core::State {
                id: "state.opening".into(),
                name: "opening".into(),
                story_id: "story.y".into(),
                scenes: Vec::new(),
            },
        ),
    ]);
    let actions = BTreeMap::from([(
        "title.start".to_string(),
        astra_vn_core::SystemActionProgram {
            id: "title.start".into(),
            effects: vec![astra_vn_core::SystemActionEffect::Jump {
                target: "opening".into(),
            }],
        },
    )]);
    let system_story_ids = BTreeSet::from(["system.title"]);

    assert_eq!(
        system_action_gameplay_entry_states(&states, &actions, &system_story_ids),
        vec!["state.opening".to_string()]
    );
}

#[test]
fn image_prewarm_starts_with_the_runtime_launch_state() {
    let stories = vec![
        astra_vn_core::Story {
            id: "story.extra".into(),
            name: "extra".into(),
            states: vec!["state.extra".into()],
        },
        astra_vn_core::Story {
            id: "story.main".into(),
            name: "main".into(),
            states: vec!["state.title".into(), "state.prologue".into()],
        },
    ];
    assert_eq!(
        default_runtime_launch_state(&stories),
        Some("state.prologue".to_string())
    );
}

#[test]
fn retained_pointer_activation_prefers_the_topmost_enabled_actionable_node() {
    let bounds = astra_ui_core::UiRect {
        min: astra_ui_core::UiPoint { x: 0.0, y: 0.0 },
        max: astra_ui_core::UiPoint { x: 100.0, y: 100.0 },
    };
    let node = |id: &str, actions: BTreeSet<astra_ui_core::UiSemanticAction>| {
        astra_ui_core::UiSemanticNode {
            id: id.to_string(),
            parent_id: None,
            role: astra_ui_core::UiSemanticRole::Button,
            bounds_points: bounds,
            name: None,
            description: None,
            value: None,
            enabled: true,
            hidden: false,
            focused: false,
            selected: false,
            checked: None,
            actions,
            properties: BTreeMap::new(),
        }
    };
    let snapshot = astra_ui_core::UiSemanticSnapshot {
        schema: "astra.ui_semantics.v1".to_string(),
        session_id: "test".to_string(),
        generation: 1,
        root_id: "root".to_string(),
        nodes: vec![
            node("root", BTreeSet::new()),
            node(
                "root/advance",
                BTreeSet::from([astra_ui_core::UiSemanticAction::Activate]),
            ),
        ],
    };
    assert_eq!(
        retained_pointer_activation_target(&snapshot, astra_ui_core::UiPoint { x: 50.0, y: 50.0 },)
            .map(|node| node.id.as_str()),
        Some("root/advance")
    );
}

fn packaged_families() -> Vec<String> {
    vec![
        "Noto Sans JP".to_string(),
        "Noto Sans SC".to_string(),
        "Astra Symbols".to_string(),
    ]
}

#[test]
fn locale_selects_the_matching_cjk_font_without_dropping_fallbacks() {
    let japanese = ordered_ui_font_families(&packaged_families(), "ja");
    let simplified_chinese = ordered_ui_font_families(&packaged_families(), "zh-Hans");

    assert_eq!(japanese[0], "Noto Sans JP");
    assert_eq!(simplified_chinese[0], "Noto Sans SC");
    assert_eq!(japanese.len(), 3);
    assert_eq!(simplified_chinese.len(), 3);
}

#[test]
fn non_cjk_locale_uses_the_packaged_japanese_baseline_deterministically() {
    let first = ordered_ui_font_families(&packaged_families(), "en");
    let second = ordered_ui_font_families(&packaged_families(), "en");

    assert_eq!(first[0], "Noto Sans JP");
    assert_eq!(first, second);
}

#[test]
fn decoded_cache_partition_preserves_the_profile_bound() {
    let budget =
        NativeVnDecodedCacheBudget::partition(DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES).unwrap();
    assert_eq!(budget.asset_bytes, 48 * 1024 * 1024);
    assert_eq!(budget.audio_bytes, 143 * 1024 * 1024);
    assert_eq!(budget.glyph_bytes, 1024 * 1024);
    assert_eq!(
        budget.asset_bytes + budget.audio_bytes + budget.glyph_bytes,
        DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES
    );
    assert!(NativeVnDecodedCacheBudget::partition(3).is_err());
}

#[test]
fn semantic_text_alignment_uses_bounded_container_offsets() {
    assert_eq!(UiTextAlignment::Start.offset(754, 320), 0);
    assert_eq!(UiTextAlignment::Center.offset(754, 320), 217);
    assert_eq!(UiTextAlignment::End.offset(754, 320), 434);
    assert_eq!(UiTextAlignment::Center.offset(320, 754), 0);
}

#[test]
fn invalid_semantic_text_alignment_is_blocking() {
    let invalid = "middle".to_string();
    let error = parse_ui_text_alignment(Some(&invalid)).unwrap_err();
    assert!(error.to_string().contains("ASTRA_PLAYER_UI_TEXT_ALIGNMENT"));
}

#[test]
fn save_slot_view_model_is_created_only_from_the_bound_profile_policy() {
    let policy = SystemUiProfilePolicy {
        profile_id: "classic".into(),
        save_slot_ids: (1..=8).map(|index| format!("slot.{index:02}")).collect(),
        quick_slot_id: None,
        allowed_pages: BTreeSet::from([SystemPageKind::Save, SystemPageKind::Load]),
        reading_modes: BTreeSet::from([ReadingMode::Manual]),
        audio_toggle: true,
        save_completion: SaveCompletionPolicy::ReturnSystem,
        custom_action_ids: BTreeSet::new(),
    };
    let slots = save_slots_for_policy(&policy);
    assert_eq!(slots.len(), 8);
    assert!(slots.contains_key("slot.01"));
    assert!(slots.contains_key("slot.08"));
    assert!(!slots.contains_key("slot.09"));
    assert!(!slots.contains_key("slot.quick"));
}

#[test]
fn frame_localization_projects_only_static_and_live_model_keys() {
    let dictionary = BTreeMap::from([
        ("ui.title".into(), "Title".into()),
        ("story.current".into(), "Current line".into()),
        ("story.unused".into(), "Unused line".into()),
    ]);
    let view_keys = BTreeMap::from([("view.main".into(), BTreeSet::from(["ui.title".into()]))]);
    let model = UiValue::Map(BTreeMap::from([(
        "text".into(),
        UiValue::String("story.current".into()),
    )]));
    let projected = frame_localization_subset(
        &dictionary,
        &view_keys,
        "view.main",
        &model,
        &UiValue::Null,
        &[],
    )
    .unwrap();
    assert_eq!(projected.len(), 2);
    assert_eq!(projected["ui.title"], "Title");
    assert_eq!(projected["story.current"], "Current line");
    assert!(!projected.contains_key("story.unused"));
}

#[test]
fn frame_localization_rejects_missing_static_or_modal_projection() {
    let dictionary = BTreeMap::new();
    let view_keys = BTreeMap::from([("view.main".into(), BTreeSet::from(["ui.missing".into()]))]);
    let error = frame_localization_subset(
        &dictionary,
        &view_keys,
        "view.main",
        &UiValue::Null,
        &UiValue::Null,
        &[],
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_PLAYER_UI_LOCALIZATION_MISSING"));

    let modal = UiBlueprintModalFrameModel {
        view_id: "view.modal".into(),
        model_schema: "ui.modal.v1".into(),
        model: UiValue::Null,
        state: UiValue::Null,
    };
    let error = frame_localization_subset(
        &BTreeMap::new(),
        &BTreeMap::from([("view.main".into(), BTreeSet::new())]),
        "view.main",
        &UiValue::Null,
        &UiValue::Null,
        &[modal],
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_PLAYER_UI_LOCALIZATION_VIEW_MISSING"));
}
