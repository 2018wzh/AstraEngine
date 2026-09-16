use super::*;
use rfvp::subsystem::resources::{
    graph_buff::GraphBuff,
    motion_manager::{DissolveType, MotionManager},
};

#[test]
#[ignore = "requires a hardware GPU and installed native system fonts"]
fn native_text_restore_rebuilds_the_same_gpu_surface() {
    use rfvp::{
        script::parser::Nls,
        subsystem::resources::{color_manager::ColorItem, text_manager::FontEnumerator, vfs::Vfs},
    };
    let fonts = FontEnumerator::from_system_font_bindings(
        crate::font_bindings::load_system_font_bindings().unwrap(),
    )
    .unwrap();
    let vfs = Vfs::new(Nls::ShiftJIS).unwrap();
    let mut renderer = GpuRenderer::new(256, 64).unwrap();
    let mut motion = MotionManager::new();
    let mut setup = motion.capture_snapshot_v2();
    let prim = &mut setup.prim_manager.prims[0];
    prim.typ = 5;
    prim.draw_flag = true;
    prim.alpha = 255;
    prim.text_index = 0;
    prim.texture_id = 4064;
    let text = &mut setup.text_manager.items[0];
    text.text_content = "Restore".into();
    text.content_text = "Restore".into();
    text.w = 256;
    text.h = 64;
    text.loaded = true;
    text.total_chars = 7;
    text.visible_chars = 7;
    text.text_size1 = 32;
    text.text_font_idx1 = rfvp::subsystem::resources::text_manager::FONTFACE_MS_GOTHIC;
    text.text_font_idx2 = rfvp::subsystem::resources::text_manager::FONTFACE_MS_GOTHIC;
    text.outline_size1 = 5;
    text.color1 = ColorItem::white();
    text.color2 = ColorItem::black();
    text.color2.set_b(180);
    motion.apply_snapshot_v2(&setup, &vfs).unwrap();
    motion.text_upload_slot(0, &fonts, true).unwrap();
    let before = renderer.0.render(&motion).unwrap();
    assert!(before.as_chunks::<4>().0.iter().any(|p| p[2] > p[0]));
    let saved = motion.capture_snapshot_v2();
    motion.apply_snapshot_v2(&saved, &vfs).unwrap();
    motion.text_upload_slot(0, &fonts, true).unwrap();
    assert_eq!(renderer.0.render(&motion).unwrap(), before);
    let mut cold = MotionManager::new();
    cold.apply_snapshot_v2(&saved, &vfs).unwrap();
    cold.text_upload_slot(0, &fonts, true).unwrap();
    let mut cold_renderer = GpuRenderer::new(256, 64).unwrap();
    assert_eq!(cold_renderer.0.render(&cold).unwrap(), before);
    cold.set_alpha_motion(
        0,
        255,
        0,
        1000,
        rfvp::subsystem::resources::motion_manager::AlphaMotionType::Linear,
        false,
    )
    .unwrap();
    cold.update_alpha_motions(200, false);
    let fading = cold.capture_snapshot_v2();
    let middle = cold_renderer.0.render(&cold).unwrap();
    assert_ne!(middle, before);
    let mut resumed = MotionManager::new();
    resumed.apply_snapshot_v2(&fading, &vfs).unwrap();
    resumed.text_upload_slot(0, &fonts, true).unwrap();
    assert_eq!(renderer.0.render(&resumed).unwrap(), middle);
    cold.update_alpha_motions(800, false);
    resumed.update_alpha_motions(800, false);
    assert_eq!(
        renderer.0.render(&resumed).unwrap(),
        cold_renderer.0.render(&cold).unwrap()
    );
    assert!(!resumed.test_alpha_motion(0));
}

#[test]
#[ignore = "requires a hardware GPU"]
fn native_restore_preserves_graph_tone_and_gpu_pixels() {
    use rfvp::{script::parser::Nls, subsystem::resources::vfs::Vfs};
    let mut vfs = Vfs::new(Nls::ShiftJIS).unwrap();
    let mut renderer = GpuRenderer::new(4, 2).unwrap();
    let mut motion = MotionManager::new();
    let mut initial = motion.capture_snapshot_v2();
    let prim = &mut initial.prim_manager.prims[0];
    prim.typ = 4;
    prim.draw_flag = true;
    prim.alpha = 255;
    prim.texture_id = 0;
    motion.apply_snapshot_v2(&initial, &vfs).unwrap();
    let source = white_nvsg();
    vfs.add_loose_file("white.nvsg", source.clone());
    motion.load_graph(0, "white.nvsg", source).unwrap();
    motion.graph_color_tone(0, 50, 25, 75);
    let before = renderer.0.render(&motion).unwrap();
    assert!(before
        .as_chunks::<4>()
        .0
        .iter()
        .any(|p| p[0] > 0 && p[0] < p[2]));
    let saved = motion.capture_snapshot_v2();
    assert!(saved.textures[0].texture_path.is_empty());
    assert!(saved.textures[0].rgba.is_some());
    motion.graph_color_tone(0, 0, 0, 0);
    assert_ne!(renderer.0.render(&motion).unwrap(), before);
    motion.apply_snapshot_v2(&saved, &vfs).unwrap();
    assert_eq!(renderer.0.render(&motion).unwrap(), before);
    let mut cold = MotionManager::new();
    cold.apply_snapshot_v2(&saved, &vfs).unwrap();
    assert_eq!(renderer.0.render(&cold).unwrap(), before);
}

fn white_nvsg() -> Vec<u8> {
    let mut bytes = b"hzc1".to_vec();
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.extend_from_slice(b"NVSG");
    for field in [0u16, 1, 4, 2, 0, 0, 0, 0] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 12]);
    // zlib-compressed 4x2 opaque white BGRA pixels.
    bytes.extend_from_slice(&[120, 156, 251, 255, 31, 63, 0, 0, 14, 46, 31, 225]);
    bytes
}

#[test]
fn source_graph_restore_keeps_placement_metadata() {
    use rfvp::{script::parser::Nls, subsystem::resources::vfs::Vfs};
    let mut vfs = Vfs::new(Nls::ShiftJIS).unwrap();
    let source = white_nvsg();
    vfs.add_loose_file("white.nvsg", source.clone());
    let mut graph = GraphBuff::new();
    graph.load_texture("white.nvsg", source).unwrap();
    graph.offset_x = 3;
    graph.offset_y = 5;
    graph.display_width = 8;
    graph.display_height = 4;
    graph.u = 1;
    graph.v = 1;
    let snapshot = graph.capture_snapshot_with_id(0);
    let generation = graph.generation;
    graph.apply_snapshot_v1(&snapshot, &vfs).unwrap();
    assert_eq!(
        (graph.offset_x, graph.offset_y, graph.u, graph.v),
        (3, 5, 1, 1)
    );
    assert_eq!((graph.display_width, graph.display_height), (8, 4));
    assert!(graph.generation > generation);
}

#[test]
#[ignore = "requires a hardware GPU"]
fn native_mask_dissolve_preserves_previous_frame_and_thresholds() {
    let mut renderer = GpuRenderer::new(4, 1).unwrap();
    let mut motion = MotionManager::new();
    motion.set_dissolve_color_id(2);
    motion.set_dissolve_type(DissolveType::Static);
    motion.tick_dissolve(0);
    assert_eq!(renderer.0.render(&motion).unwrap(), vec![255; 16]);
    let mut graph = GraphBuff::new();
    graph.texture = Some(Arc::new(rfvp::DynamicImage::ImageLumaA8(
        rfvp::GrayAlphaImage::from_raw(4, 1, vec![255, 0, 255, 64, 255, 128, 255, 255]).unwrap(),
    )));
    motion.set_dissolve_mask_graph(graph);
    motion.start_dissolve(255, DissolveType::MaskFadeIn);
    motion.tick_dissolve(128);
    let pixels = renderer.0.render(&motion).unwrap();
    assert_eq!(
        pixels,
        [0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255]
    );
    motion.tick_dissolve(100);
    let pixels = renderer.0.render(&motion).unwrap();
    assert_eq!(&pixels[8..12], &[0, 0, 0, 255]);
    assert_eq!(&pixels[12..], &[255; 4]);
    motion.tick_dissolve(27);
    assert!(renderer
        .0
        .render(&motion)
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [0, 0, 0, 255]));

    // A new transition captures a new frame and uses both halves of its phase.
    motion.set_dissolve_type(DissolveType::Static);
    motion.tick_dissolve(0);
    assert_eq!(renderer.0.render(&motion).unwrap(), vec![255; 16]);
    motion.start_dissolve(510, DissolveType::MaskFadeInOut);
    motion.tick_dissolve(255);
    let pixels = renderer.0.render(&motion).unwrap();
    assert_eq!(&pixels[..4], &[0, 0, 0, 255]);
    assert_eq!(&pixels[12..], &[255; 4]);
    assert!(pixels[4] < pixels[8]);
    motion.tick_dissolve(128);
    let pixels = renderer.0.render(&motion).unwrap();
    assert_eq!(&pixels[8..12], &[0, 0, 0, 255]);
    assert!(pixels[12] > 128 && pixels[12] < 255);
    motion.tick_dissolve(127);
    assert!(renderer
        .0
        .render(&motion)
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [0, 0, 0, 255]));
}
