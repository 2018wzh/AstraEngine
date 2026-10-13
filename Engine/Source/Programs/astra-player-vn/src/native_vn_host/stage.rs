use super::ui_layout::*;

use super::*;

#[cfg(test)]
mod director_transition_tests {
    use super::{super::DirectorTransitionSnapshot, compose_director_transition_scene};
    use astra_media_core::SceneCommand;
    use astra_vn_package::{
        AspectRatio, FixedScalar, ProductStageCamera, ProductStageState, StageViewport,
        PRODUCT_STAGE_STATE_SCHEMA,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn source_state() -> ProductStageState {
        ProductStageState {
            schema: PRODUCT_STAGE_STATE_SCHEMA.into(),
            profile: "modern".into(),
            configured: false,
            viewport: StageViewport {
                width: 800,
                height: 600,
            },
            safe_area: AspectRatio {
                width: 16,
                height: 9,
            },
            preloaded_assets: BTreeSet::new(),
            layers: BTreeMap::new(),
            entities: BTreeMap::new(),
            camera: ProductStageCamera::default(),
            movies: BTreeMap::new(),
            effects: BTreeMap::new(),
            backdrop_color: None,
            shade_color: [0, 0, 0, 255],
            shade_opacity: FixedScalar::ZERO,
            skip_allowed: true,
            transition: None,
            audio_bus_enabled: BTreeMap::new(),
            frame_index: 0,
            elapsed_ns: 0,
        }
    }

    fn next_scene() -> Vec<SceneCommand> {
        vec![SceneCommand::rect(
            "next",
            0,
            0,
            800,
            600,
            [255, 255, 255, 255],
        )]
    }

    #[test]
    fn wipe_transition_clips_the_new_scene_by_fixed_progress() {
        let snapshot = DirectorTransitionSnapshot {
            descriptor_id: "director.puppet.1".into(),
            source_state: source_state(),
            source_draw: vec![SceneCommand::rect(
                "previous",
                0,
                0,
                800,
                600,
                [0, 0, 0, 255],
            )],
        };
        let composed =
            compose_director_transition_scene(&snapshot, &next_scene(), 500_000, 800, 600)
                .expect("compose wipe");
        assert!(matches!(
            composed[1],
            SceneCommand::PushClip { rect } if rect.width == 400 && rect.height == 600
        ));
        assert!(matches!(composed.last(), Some(SceneCommand::PopClip)));
    }

    #[test]
    fn transition_namespaces_outgoing_and_each_incoming_draw_instance() {
        let snapshot = DirectorTransitionSnapshot {
            descriptor_id: "director.puppet.26".into(),
            source_state: source_state(),
            source_draw: vec![SceneCommand::rect(
                "vn.scene.backdrop",
                0,
                0,
                800,
                600,
                [0, 0, 0, 255],
            )],
        };
        let next = vec![SceneCommand::rect(
            "vn.scene.backdrop",
            0,
            0,
            800,
            600,
            [255, 255, 255, 255],
        )];
        let composed = compose_director_transition_scene(&snapshot, &next, 500_000, 32, 32)
            .expect("compose dissolve");
        let ids = composed
            .iter()
            .filter_map(|command| match command {
                SceneCommand::Rect { id, .. } => Some(id),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 2, "the incoming scene must be emitted only once");
        assert_eq!(ids.len(), ids.iter().collect::<BTreeSet<_>>().len());
        assert!(ids.iter().all(|id| id.starts_with("vn.transition.")));
    }

    #[test]
    fn dissolve_patterns_uses_the_fixed_native_pixel_mask() {
        let snapshot = DirectorTransitionSnapshot {
            descriptor_id: "director.puppet.26".into(),
            source_state: source_state(),
            source_draw: Vec::new(),
        };
        let first = compose_director_transition_scene(&snapshot, &next_scene(), 500_000, 32, 32)
            .expect("compose dissolve");
        let second = compose_director_transition_scene(&snapshot, &next_scene(), 500_000, 32, 32)
            .expect("compose dissolve repeat");
        assert_eq!(first, second);
        let SceneCommand::PushPixelMask { bits } = first[0] else {
            panic!("missing mask")
        };
        assert_eq!(bits, 0xaa55aa55aa55aa55);
        assert_eq!(first.len(), next_scene().len() + 2);
        let large =
            compose_director_transition_scene(&snapshot, &next_scene(), 500_000, 2560, 1440)
                .expect("compose large dissolve");
        assert_eq!(
            first, large,
            "command size must not scale with viewport pixels"
        );
    }
}

mod validate_product_provider_bindings;
pub(super) use validate_product_provider_bindings::cleanup_runtime_host;
pub(super) use validate_product_provider_bindings::current_image_working_set;
pub(super) use validate_product_provider_bindings::default_runtime_launch_state;
pub(super) use validate_product_provider_bindings::image_prefetch_windows;
pub(super) use validate_product_provider_bindings::player_timeline_task;
pub(super) use validate_product_provider_bindings::saved_runtime_state;
#[cfg(test)]
pub(super) use validate_product_provider_bindings::state_prefetch_successors;
#[cfg(test)]
pub(super) use validate_product_provider_bindings::system_action_gameplay_entry_states;
pub(super) use validate_product_provider_bindings::unique_string_set;
pub(super) use validate_product_provider_bindings::validate_product_provider_bindings;
mod stage_scene_commands;
pub(super) use stage_scene_commands::compose_director_transition_scene;
pub(super) use stage_scene_commands::restore_director_transition_snapshot;
pub(super) use stage_scene_commands::save_director_transition_snapshot;
pub(super) use stage_scene_commands::stage_scene_commands;
