#![cfg(all(target_os = "windows", feature = "platform-test-driver"))]

use astra_media::{FilterGraph, FilterNode, FilterParam, FilterTarget, SceneCommand};
use astra_platform::{
    PlatformHostFactory, PlatformHostProfile, SceneFrame, SurfaceRequest, WindowRequest,
};
use std::collections::BTreeMap;

#[tokio::test]
async fn window_filter_matches_offscreen_after_resize_and_device_recovery() {
    let host = astra_platform_windows::factory()
        .start(astra_platform::HostLaunchProfile::platform(
            PlatformHostProfile::windows_release("nativevn-game", "com.example.filters"),
        ))
        .await
        .unwrap();
    let window = host
        .client
        .create_window(WindowRequest {
            title: "Astra Filter Parity".into(),
            width: 64,
            height: 64,
            visible: true,
        })
        .await
        .unwrap();
    let surface = host
        .client
        .create_surface(SurfaceRequest {
            window,
            width: 64,
            height: 64,
        })
        .await
        .unwrap();
    let mut offscreen = astra_platform_common::WgpuOffscreenRenderer::new(
        &astra_platform_common::native_wgpu_instance().unwrap(),
    )
    .await
    .unwrap();
    for sequence in 1..=3 {
        let side = if sequence == 1 { 64 } else { 96 };
        let frame = SceneFrame {
            sequence,
            width: side,
            height: side,
            clear_rgba: [255; 4],
            semantics: None,
            commands: vec![SceneCommand::FilterGraph {
                graph: FilterGraph {
                    schema: "astra.filter_graph.v1".into(),
                    nodes: vec![FilterNode {
                        id: "fade".into(),
                        kind: "astra.filter.fade".into(),
                        input: FilterTarget::Final,
                        output: FilterTarget::Final,
                        params: BTreeMap::from([("amount".into(), FilterParam::Float(0.25))]),
                        deterministic: true,
                        allow_cpu_fallback: false,
                    }],
                },
            }],
        };
        if sequence == 3 {
            host.client
                .inject_surface_device_loss(surface)
                .await
                .unwrap();
            let error = host
                .client
                .present_scene(surface, frame.clone())
                .await
                .unwrap_err();
            assert_eq!(error.code, astra_platform::PlatformErrorCode::DeviceLost);
        }
        let expected = offscreen.render(&frame).unwrap();
        host.client.present_scene(surface, frame).await.unwrap();
        let actual = host.client.capture_surface(surface).await.unwrap();
        assert_eq!(actual.rgba8, expected.rgba8);
        assert!(actual
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0].abs_diff(137) <= 1 && p[1] == p[0] && p[2] == p[0] && p[3] == 255));
    }
    host.client.destroy_surface(surface).await.unwrap();
    host.client.destroy_window(window).await.unwrap();
    host.client.shutdown().await.unwrap();
}
