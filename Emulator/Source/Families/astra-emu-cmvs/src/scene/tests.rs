use super::*;
use crate::CmvsTextureParentState;

fn state() -> CmvsPs2aVmState {
    let mut vm = CmvsPs2aVmState::new(0);
    for (slot, z) in [(0, 2), (1, 1)] {
        vm.texture_parents.insert(
            slot,
            CmvsTextureParentState {
                surface_initialized: true,
                rect_words: Some([0, 0, 2, 2]),
                position_words: Some([0, 0]),
                auxiliary_word: Some(z),
                ..Default::default()
            },
        );
    }
    vm
}

#[test]
fn recovered_geometry_orders_layers_and_rejects_missing_or_overflowing_state() {
    let mut vm = state();
    let slots = [CmvsTextureSlot::Parent(0), CmvsTextureSlot::Parent(1)];
    let geometry = stage_geometry(&vm, slots.into_iter(), slots[0]).unwrap();
    assert_eq!(geometry[0].0, slots[1]);
    vm.texture_parents.get_mut(&0).unwrap().position_words = Some([u32::MAX, 0]);
    assert_eq!(
        stage_geometry(&vm, slots.into_iter(), slots[0])
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_TEXTURE_GEOMETRY"
    );
    vm.texture_parents.get_mut(&0).unwrap().position_words = None;
    assert_eq!(
        stage_geometry(&vm, slots.into_iter(), slots[0])
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_TEXTURE_PRESENTATION_STATE"
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_composes_recovered_layer_order_and_replaces_texture_sizes() {
    let mut scene = CmvsScene::new(gpu_instance()).unwrap();
    let vm = state();
    let slots = [CmvsTextureSlot::Parent(0), CmvsTextureSlot::Parent(1)];
    for size in [1, 2, 1] {
        let commands = stage_geometry(&vm, slots.into_iter(), slots[0])
            .unwrap()
            .into_iter()
            .map(|(slot, destination, _)| {
                let color = if slot == slots[0] {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                };
                SceneCommand::Texture {
                    id: slot_id(slot),
                    frame: TextureFrame {
                        width: size,
                        height: size,
                        rgba8: color.repeat((size * size) as usize).into(),
                    },
                    destination,
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                }
            })
            .collect();
        let frame = scene.draw(2, 2, commands).unwrap();
        assert!(frame
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255, 0, 0, 255]));
    }
    assert_eq!(scene.sequence, 3);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_raster_resize_keeps_stage_geometry_and_releases_each_renderer() {
    let instance = gpu_instance();
    for _ in 0..32 {
        let mut scene = CmvsScene::new(instance).unwrap();
        let mut failed_candidate = CmvsScene::new(instance).unwrap();
        assert!(failed_candidate.draw(0, 2, Vec::new()).is_err());
        drop(failed_candidate);
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
        for (width, height) in [(6, 4), (4, 6), (2, 2)] {
            scene.resize_raster(width, height).unwrap();
            let frame = scene
                .draw(
                    2,
                    2,
                    vec![SceneCommand::Texture {
                        id: "cmvs.test.stage".into(),
                        frame: TextureFrame {
                            width: 1,
                            height: 1,
                            rgba8: vec![255, 0, 0, 255].into(),
                        },
                        destination: RectI {
                            x: 0,
                            y: 0,
                            width: 2,
                            height: 2,
                        },
                        opacity: 1.0,
                        blend: BlendMode::Alpha,
                    }],
                )
                .unwrap();
            assert_eq!((frame.width, frame.height), (width, height));
            assert_eq!(scene.logical_extent(), Some(Extent2D::new(2, 2)));
            let before_canvas = scene.canvas;
            let before_raster = scene.raster;
            let before_sequence = scene.sequence;
            for (bad_width, bad_height) in [(0, height), (width, 0), (u32::MAX, height)] {
                assert!(scene.resize_raster(bad_width, bad_height).is_err());
                assert_eq!(scene.canvas, before_canvas);
                assert_eq!(scene.raster, before_raster);
                assert_eq!(scene.sequence, before_sequence);
            }
            let viewport = scene.canvas.unwrap().viewport();
            for y in 0..height {
                for x in 0..width {
                    let offset = ((y * width + x) * 4) as usize;
                    let inside = x >= viewport.x
                        && x < viewport.x + viewport.width
                        && y >= viewport.y
                        && y < viewport.y + viewport.height;
                    assert_eq!(
                        &frame.rgba8[offset..offset + 4],
                        if inside {
                            &[255, 0, 0, 255]
                        } else {
                            &[0, 0, 0, 255]
                        }
                    );
                }
            }
        }
        drop(scene);
        assert!(instance.poll_all(true));
        let report = instance.generate_report().unwrap();
        for (name, registry) in [
            ("adapters", &report.hub.adapters),
            ("devices", &report.hub.devices),
            ("queues", &report.hub.queues),
            ("buffers", &report.hub.buffers),
            ("textures", &report.hub.textures),
            ("texture_views", &report.hub.texture_views),
            ("bind_group_layouts", &report.hub.bind_group_layouts),
            ("bind_groups", &report.hub.bind_groups),
            ("pipeline_layouts", &report.hub.pipeline_layouts),
            ("shader_modules", &report.hub.shader_modules),
            ("command_encoders", &report.hub.command_encoders),
            ("command_buffers", &report.hub.command_buffers),
            ("render_bundles", &report.hub.render_bundles),
            ("render_pipelines", &report.hub.render_pipelines),
            ("compute_pipelines", &report.hub.compute_pipelines),
            ("pipeline_caches", &report.hub.pipeline_caches),
            ("query_sets", &report.hub.query_sets),
            ("external_textures", &report.hub.external_textures),
            ("samplers", &report.hub.samplers),
            ("surfaces", &report.surfaces),
        ] {
            assert!(registry.is_empty(), "retained {name}: {registry:?}");
        }
    }
}

fn gpu_instance() -> &'static wgpu::Instance {
    static INSTANCE: std::sync::OnceLock<wgpu::Instance> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(|| astra_platform_common::native_wgpu_instance().unwrap())
}
