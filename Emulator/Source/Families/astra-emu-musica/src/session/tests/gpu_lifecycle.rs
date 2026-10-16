use super::*;

#[test]
fn native_gpu_one_provider_reopens_32_sessions_and_releases_each_device() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".stage * BG.png 0 0\r\n.message 1   Hello\r\n.end\r\n",
    );
    // Exercise the production lazy owner; do not inject the suite's Instance.
    let mut provider = crate::MusicaProvider::default();
    for _ in 0..32 {
        let (_, mut session) = provider
            .open_session(request(root.path(), Sink::default()))
            .unwrap();
        let instance = session.scene.gpu_instance().clone();
        session.advance(16_666_667, &[]).unwrap();
        let FramePoll::Ready(frame) = session.acquire_frame(None).unwrap() else {
            panic!("first native frame is not ready");
        };
        assert_eq!(
            instance
                .generate_report()
                .unwrap()
                .hub
                .devices
                .num_kept_from_user,
            1
        );
        Box::new(session).close().unwrap();
        assert_session_resources_released(&instance);
        // A real published CPU lease must not keep the session GPU alive.
        assert_eq!(&frame.pixels()[..4], &[25, 100, 220, 255]);
        // Only the provider may retain this Instance between open calls.
        drop(instance);
    }
}

#[test]
fn native_gpu_restore_candidate_failure_releases_only_the_candidate_device() {
    let _session = PROVIDER_SESSION.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1   Hello\r\n.end\r\n");
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    session.save(20).unwrap();
    let before_vm = session.vm.encode_native_save().unwrap();
    let before_pixels = session.scene.pixels.clone();
    let mut invalid = session.storage.read(20).unwrap();
    // This fails while rendering the new Scene, after its device was created.
    invalid.message = Some(("\u{10ffff}".into(), None));
    session.storage.write(21, &invalid).unwrap();
    assert_eq!(
        session.load(21).unwrap_err().code(),
        "ASTRA_TEXT_GLYPH_MISSING"
    );
    assert_eq!(session.vm.encode_native_save().unwrap(), before_vm);
    assert_eq!(session.scene.pixels, before_pixels);
    assert_eq!(session.message.as_ref().unwrap().0, "Hello");
    let instance = fixture::gpu_instance();
    assert!(instance.poll_all(true));
    assert_eq!(
        instance
            .generate_report()
            .unwrap()
            .hub
            .devices
            .num_kept_from_user,
        1
    );
    session.load(20).unwrap();
    assert_eq!(session.scene.pixels, before_pixels);
    assert_eq!(
        instance
            .generate_report()
            .unwrap()
            .hub
            .devices
            .num_kept_from_user,
        1
    );
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert!(session.finished);
    Box::new(session).close().unwrap();
    assert_session_resources_released(instance);
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
