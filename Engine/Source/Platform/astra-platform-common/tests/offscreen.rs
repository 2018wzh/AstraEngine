use std::collections::BTreeMap;

use astra_core::Hash256;
use astra_media_core::{
    FilterGraph, FilterNode, FilterParam, FilterTarget, SceneCommand, TextureFrame,
};
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
    let mut frame = SceneFrame {
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
    };
    let capture = renderer.render(&frame).unwrap();
    assert_eq!((capture.width, capture.height), (4, 4));
    assert_eq!(capture.rgba8.len(), 64);
    assert!(capture.rgba8.chunks_exact(4).any(|pixel| pixel[3] != 0));
    assert_eq!(renderer.performance_counters().readback_bytes, 64);
    for sequence in 2..=4 {
        frame.sequence = sequence;
        renderer.submit_frame(&frame).unwrap();
        assert_eq!(renderer.capture_checkpoint().unwrap().rgba8, capture.rgba8);
    }
}

#[tokio::test]
#[ignore = "requires the Windows integrated-GPU performance runner"]
async fn integrated_gpu_timestamp_profile_has_zero_stable_atlas_upload() {
    let policy = astra_platform::GpuAdapterPolicy {
        backend: astra_platform::GpuBackendPolicy::Dx12,
        device_type: astra_platform::GpuDeviceTypePolicy::Integrated,
        require_timestamp_query: true,
        adapter_identity_hash: None,
    };
    let mut renderer = WgpuOffscreenRenderer::new_with_policy(&policy)
        .await
        .unwrap();
    let mut frame = SceneFrame {
        sequence: 1,
        width: 64,
        height: 64,
        clear_rgba: [0, 0, 0, 255],
        commands: vec![SceneCommand::rect(
            "performance.rect",
            0,
            0,
            64,
            64,
            [255; 4],
        )],
        semantics: None,
    };
    let first = renderer.submit_frame_profiled(&frame).unwrap();
    assert!(first.gpu_duration_ns > 0);
    assert!(first.atlas_upload_gpu_ns > 0);
    assert!(renderer.performance_counters().upload_bytes > 0);
    frame.sequence += 1;
    let second = renderer.submit_frame_profiled(&frame).unwrap();
    assert!(second.gpu_duration_ns > 0);
    assert_eq!(second.atlas_upload_gpu_ns, 0);
    assert_eq!(renderer.performance_counters().upload_bytes, 0);

    let pixels = [12_u8, 34, 56, 255].repeat(8 * 8);
    frame.sequence += 1;
    frame.commands.insert(
        0,
        SceneCommand::UploadTexture {
            resource_id: "performance.incremental".into(),
            frame: TextureFrame {
                width: 8,
                height: 8,
                hash: Hash256::from_sha256(&pixels),
                rgba8: pixels.into(),
            },
        },
    );
    let incremental = renderer.submit_frame_profiled(&frame).unwrap();
    assert!(incremental.atlas_upload_gpu_ns > 0);
    let counters = renderer.performance_counters();
    // The incremental copy includes the atlas' one-pixel sampling guard on every edge.
    assert_eq!(counters.upload_bytes, (8 + 2) * (8 + 2) * 4);
    assert_eq!(counters.queue_submissions, 3);

    frame.sequence += 1;
    frame.commands.remove(0);
    let stable = renderer.submit_frame_profiled(&frame).unwrap();
    assert_eq!(stable.atlas_upload_gpu_ns, 0);
    let counters = renderer.performance_counters();
    assert_eq!(counters.upload_bytes, 0);
    assert_eq!(counters.queue_submissions, 2);
}
