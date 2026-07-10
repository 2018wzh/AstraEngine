use astra_core::Hash256;
use astra_media_core::{FilterGraph, FilterNode, FilterParam, FilterTarget};
use astra_vn_presentation::{
    LayerKind, StageModel, StandardPresentationCommand, TextWindowState,
    VnHeadlessPresentationExecutor, VnPresentationAsset, VnPresentationExecutionRequest,
};

fn asset(asset_id: &str, rgba: [u8; 4]) -> VnPresentationAsset {
    let bytes = rgba.repeat(4);
    VnPresentationAsset {
        asset_id: asset_id.to_string(),
        format: "rgba8_srgb".to_string(),
        width: 2,
        height: 2,
        hash: Hash256::from_sha256(&bytes),
        bytes,
    }
}

#[test]
fn headless_presentation_executor_renders_stage_and_runs_filter_graph() {
    let mut stage = StageModel::new(320, 180);
    stage.apply(StandardPresentationCommand::ShowLayer {
        id: "bg.school".to_string(),
        kind: LayerKind::Background,
        asset: "native-assets/background/school.png".to_string(),
        z: 0,
        x: 0.0,
        y: 0.0,
    });
    stage.apply(StandardPresentationCommand::ShowLayer {
        id: "hero.main".to_string(),
        kind: LayerKind::Character,
        asset: "native-assets/character/hero_atlas.png".to_string(),
        z: 10,
        x: 120.0,
        y: 20.0,
    });
    stage.text_windows.push(TextWindowState {
        id: "dialogue".to_string(),
        x: 16.0,
        y: 128.0,
        width: 288.0,
        height: 42.0,
        visible: true,
        layout: Default::default(),
        input_priority: 0,
    });
    let filter_graph = FilterGraph {
        schema: "astra.filter_graph.v1".to_string(),
        nodes: vec![FilterNode {
            id: "classic_bloom".to_string(),
            kind: "astra.filter.bloom".to_string(),
            input: FilterTarget::Final,
            output: FilterTarget::Final,
            params: [("intensity".to_string(), FilterParam::Float(0.10))]
                .into_iter()
                .collect(),
            deterministic: true,
            allow_cpu_fallback: true,
        }],
    };

    let executor = VnHeadlessPresentationExecutor;
    let request = VnPresentationExecutionRequest {
        stage,
        assets: vec![
            asset("native-assets/background/school.png", [20, 40, 80, 255]),
            asset(
                "native-assets/character/hero_atlas.png",
                [180, 120, 100, 255],
            ),
        ],
        filters: Some(filter_graph),
        profile: "classic".to_string(),
    };
    let first = executor.execute(request.clone()).unwrap();
    let second = executor.execute(request).unwrap();

    assert_eq!(first.output_hash, second.output_hash);
    assert_ne!(first.input_hash, first.output_hash);
    assert_eq!(first.renderer_provider, "astra.renderer.headless");
    assert_eq!(first.filter_provider, "astra.media.cpu_filter_executor");
    assert_eq!(first.filter_count, 1);
    assert!(first.draw_count >= 4);
}

#[test]
fn headless_presentation_executor_blocks_invalid_filter_graph() {
    let mut stage = StageModel::new(64, 64);
    stage.apply(StandardPresentationCommand::ShowLayer {
        id: "bg.room".to_string(),
        kind: LayerKind::Background,
        asset: "native-assets/background/room.png".to_string(),
        z: 0,
        x: 0.0,
        y: 0.0,
    });
    let filter_graph = FilterGraph {
        schema: "astra.filter_graph.v1".to_string(),
        nodes: vec![FilterNode {
            id: "broken_bloom".to_string(),
            kind: "astra.filter.bloom".to_string(),
            input: FilterTarget::Final,
            output: FilterTarget::Final,
            params: [(
                "intensity".to_string(),
                FilterParam::Text("bad".to_string()),
            )]
            .into_iter()
            .collect(),
            deterministic: true,
            allow_cpu_fallback: true,
        }],
    };

    let err = VnHeadlessPresentationExecutor
        .execute(VnPresentationExecutionRequest {
            stage,
            assets: vec![asset(
                "native-assets/background/room.png",
                [40, 30, 20, 255],
            )],
            filters: Some(filter_graph),
            profile: "classic".to_string(),
        })
        .unwrap_err();

    assert_eq!(err.code(), "ASTRA_FILTER_PARAM_TYPE");
}
