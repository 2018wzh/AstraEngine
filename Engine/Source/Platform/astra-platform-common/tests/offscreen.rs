use std::collections::BTreeMap;

use astra_media_core::{FilterGraph, FilterNode, FilterParam, FilterTarget, SceneCommand};
use astra_platform::SceneFrame;
use astra_platform_common::WgpuOffscreenRenderer;

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn native_offscreen_gpu_renders_scene_filter_and_readback() {
    let mut renderer = WgpuOffscreenRenderer::new().await.unwrap();
    assert_eq!(renderer.identity().provider, "wgpu_offscreen");
    assert_eq!(
        renderer.identity().backend,
        if cfg!(target_os = "windows") {
            "dx12"
        } else if cfg!(target_os = "linux") {
            "vulkan"
        } else {
            "metal"
        }
    );
    assert_ne!(renderer.identity().device_type, "cpu");
    let mut params = BTreeMap::new();
    params.insert("amount".into(), FilterParam::Float(0.5));
    let capture = renderer
        .render(&SceneFrame {
            sequence: 1,
            width: 4,
            height: 4,
            clear_rgba: [0, 0, 0, 255],
            commands: vec![
                SceneCommand::rect("gpu.rect", 0, 0, 4, 4, [200, 100, 50, 255]),
                SceneCommand::FilterGraph {
                    graph: FilterGraph {
                        schema: "astra.filter_graph.v1".into(),
                        nodes: vec![FilterNode {
                            id: "gpu.fade".into(),
                            kind: "astra.filter.fade".into(),
                            input: FilterTarget::Final,
                            output: FilterTarget::Final,
                            params,
                            deterministic: true,
                            allow_cpu_fallback: false,
                        }],
                    },
                },
            ],
            semantics: None,
        })
        .unwrap();
    assert_eq!((capture.width, capture.height), (4, 4));
    assert_eq!(capture.rgba8.len(), 64);
    assert!(capture.rgba8.chunks_exact(4).any(|pixel| pixel[3] != 0));
}
