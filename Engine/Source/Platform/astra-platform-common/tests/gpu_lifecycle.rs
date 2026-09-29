use astra_media_core::SceneCommand;
use astra_platform::{GpuAdapterPolicy, GpuDeviceTypePolicy, SceneFrame};
use astra_platform_common::{native_wgpu_instance, WgpuOffscreenRenderer};

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn owner_keeps_instance_while_96_renderers_release_session_resources() {
    let instance = native_wgpu_instance().unwrap();
    for iteration in 0..96 {
        let mut renderer = WgpuOffscreenRenderer::new(&instance).await.unwrap();
        assert!(matches!(
            renderer.identity().device_type.as_str(),
            "integrated_gpu" | "discrete_gpu"
        ));
        let color = [iteration as u8, 100, 220, 255];
        let capture = renderer
            .render(&SceneFrame {
                sequence: 1,
                width: 4,
                height: 4,
                clear_rgba: [0; 4],
                commands: vec![SceneCommand::rect("quad", 0, 0, 4, 4, color)],
                semantics: None,
            })
            .unwrap();
        assert_eq!(&capture.rgba8[..4], &color);
        let live = instance.generate_report().expect("native wgpu report");
        assert_eq!(live.hub.devices.num_kept_from_user, 1);
        assert_eq!(live.hub.queues.num_kept_from_user, 1);
        assert!(live.hub.textures.num_kept_from_user > 0);
        drop(renderer);
        assert_session_resources_released(&instance);
    }

    let backend = if cfg!(target_os = "windows") {
        astra_platform::GpuBackendPolicy::Dx12
    } else if cfg!(target_os = "macos") {
        astra_platform::GpuBackendPolicy::Metal
    } else {
        astra_platform::GpuBackendPolicy::Vulkan
    };
    let mismatched_policy = GpuAdapterPolicy {
        backend,
        device_type: GpuDeviceTypePolicy::AnyHardware,
        require_timestamp_query: false,
        adapter_identity_hash: Some("0".repeat(64)),
    };
    assert!(
        WgpuOffscreenRenderer::new_with_policy(&instance, &mismatched_policy)
            .await
            .is_err()
    );
    let renderer = WgpuOffscreenRenderer::new(&instance).await.unwrap();
    drop(renderer);
    assert_session_resources_released(&instance);
}

fn assert_session_resources_released(instance: &wgpu::Instance) {
    assert!(instance.poll_all(true));
    let report = instance.generate_report().unwrap();
    // HubReport::is_empty only checks adapters in wgpu 29. Inspect the actual
    // registries so device, queue and session resource retention cannot hide.
    for (name, registry) in [
        ("adapters", &report.hub.adapters),
        ("devices", &report.hub.devices),
        ("queues", &report.hub.queues),
        ("pipeline_layouts", &report.hub.pipeline_layouts),
        ("shader_modules", &report.hub.shader_modules),
        ("bind_group_layouts", &report.hub.bind_group_layouts),
        ("bind_groups", &report.hub.bind_groups),
        ("command_encoders", &report.hub.command_encoders),
        ("command_buffers", &report.hub.command_buffers),
        ("render_bundles", &report.hub.render_bundles),
        ("render_pipelines", &report.hub.render_pipelines),
        ("compute_pipelines", &report.hub.compute_pipelines),
        ("pipeline_caches", &report.hub.pipeline_caches),
        ("query_sets", &report.hub.query_sets),
        ("buffers", &report.hub.buffers),
        ("textures", &report.hub.textures),
        ("texture_views", &report.hub.texture_views),
        ("external_textures", &report.hub.external_textures),
        ("samplers", &report.hub.samplers),
        ("surfaces", &report.surfaces),
    ] {
        assert!(registry.is_empty(), "retained {name}: {registry:?}");
    }
}
