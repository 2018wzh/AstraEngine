use super::{scene_texture_rgba8, texture_resource_id_for_session, YakuiPaintConverter};
use astra_ui_core::{UiTextureFormat, UiTextureId, UiTextureUpload};
use yakui_core::geometry::UVec2;
use yakui_core::paint::{Texture, TextureFormat};

#[test]
fn glyph_mask_upload_becomes_straight_alpha_scene_texture() {
    let upload = UiTextureUpload {
        id: UiTextureId(1),
        generation: 1,
        width: 2,
        height: 1,
        format: UiTextureFormat::R8Unorm,
        pixels: vec![64, 255].into(),
    };

    assert_eq!(
        scene_texture_rgba8(&upload),
        vec![255, 255, 255, 64, 255, 255, 255, 255]
    );
}

#[test]
fn premultiplied_ui_upload_is_unpremultiplied_at_scene_boundary() {
    let upload = UiTextureUpload {
        id: UiTextureId(2),
        generation: 1,
        width: 2,
        height: 1,
        format: UiTextureFormat::Rgba8SrgbPremultiplied,
        pixels: vec![100, 50, 25, 128, 0, 0, 0, 0].into(),
    };

    assert_eq!(
        scene_texture_rgba8(&upload),
        vec![199, 100, 50, 128, 0, 0, 0, 0]
    );
}

#[test]
fn texture_resource_identity_is_stable_across_ui_render_generations() {
    let first = texture_resource_id_for_session("vn.ui.demo:0", UiTextureId(7), 3);
    let repeated = texture_resource_id_for_session("vn.ui.demo:0", UiTextureId(7), 3);
    let updated = texture_resource_id_for_session("vn.ui.demo:0", UiTextureId(7), 4);

    assert_eq!(first, repeated);
    assert_ne!(first, updated);
}

#[test]
fn recreated_managed_texture_uses_explicit_lifecycle_identity() {
    let mut converter = YakuiPaintConverter::new();
    let texture = Texture::new(TextureFormat::R8, UVec2::new(2, 1), vec![42, 84]);
    let mut initial = Vec::new();
    converter
        .sync_managed_texture("ManagedTextureId(1)".into(), &texture, false, &mut initial)
        .unwrap();
    let first = converter
        .managed_textures
        .get("ManagedTextureId(1)")
        .copied()
        .unwrap();
    assert_eq!(initial.len(), 1);

    let release = converter
        .remove_managed_texture("ManagedTextureId(1)")
        .unwrap();
    let mut replacement = Vec::new();
    converter
        .sync_managed_texture(
            "ManagedTextureId(2)".into(),
            &texture,
            false,
            &mut replacement,
        )
        .unwrap();
    let second = converter
        .managed_textures
        .get("ManagedTextureId(2)")
        .copied()
        .unwrap();

    assert_ne!(first.id, second.id);
    assert_eq!(release.id, first.id);
    assert_eq!(replacement.len(), 1);
}

#[test]
fn changed_managed_texture_uploads_new_content_and_releases_old_resource() {
    let mut converter = YakuiPaintConverter::new();
    let first_texture = Texture::new(TextureFormat::R8, UVec2::new(1, 1), vec![42]);
    let second_texture = Texture::new(TextureFormat::R8, UVec2::new(1, 1), vec![84]);
    let mut initial = Vec::new();
    converter
        .sync_managed_texture(
            "ManagedTextureId(1)".into(),
            &first_texture,
            false,
            &mut initial,
        )
        .unwrap();
    let old = initial[0].id;
    let release = converter
        .remove_managed_texture("ManagedTextureId(1)")
        .unwrap();
    let mut replacement = Vec::new();
    converter
        .sync_managed_texture(
            "ManagedTextureId(1)".into(),
            &second_texture,
            false,
            &mut replacement,
        )
        .unwrap();

    assert_eq!(replacement.len(), 1);
    assert_ne!(replacement[0].id, old);
    assert_eq!(release.id, old);
}

#[test]
fn full_resync_releases_live_resources_before_reusing_their_identity() {
    let mut converter = YakuiPaintConverter::new();
    let texture = Texture::new(TextureFormat::R8, UVec2::new(1, 1), vec![42]);
    let mut initial = Vec::new();
    converter
        .sync_managed_texture("ManagedTextureId(1)".into(), &texture, false, &mut initial)
        .unwrap();
    let binding = converter
        .managed_textures
        .get("ManagedTextureId(1)")
        .copied()
        .unwrap();

    let releases = converter.release_live_textures_for_resync().unwrap();
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].id, binding.id);
    assert_eq!(releases[0].generation, binding.generation);

    let mut replay = Vec::new();
    converter
        .sync_managed_texture("ManagedTextureId(1)".into(), &texture, true, &mut replay)
        .unwrap();
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].id, binding.id);
    assert_eq!(replay[0].generation, binding.generation + 1);
}

#[test]
fn clipped_primitives_keep_balanced_scene_clips_and_unclipped_siblings() {
    use astra_media_core::{RectI, SceneCommand};
    use astra_ui_core::*;
    let primitive = |id: &str, clip_rect_points| UiMeshPrimitive {
        id: id.into(),
        layer: 0,
        clip_rect_points,
        material: UiMaterialKind::SolidColor,
        texture: None,
        texture_generation: None,
        vertices: [[0.0, 0.0], [100.0, 0.0], [0.0, 100.0]]
            .into_iter()
            .map(|position_points| UiVertex {
                position_points,
                uv: [0.0, 0.0],
                premultiplied_rgba: [255; 4],
            })
            .collect(),
        indices: vec![0, 1, 2],
    };
    let clip = UiRect {
        min: UiPoint { x: 10.0, y: 20.0 },
        max: UiPoint { x: 40.0, y: 60.0 },
    };
    let frame = UiRenderFrame {
        schema: "astra.ui_render_frame.v1".into(),
        session_id: "clip.test".into(),
        generation: 1,
        viewport: UiViewport {
            physical_width: 200,
            physical_height: 200,
            scale_factor: 2.0,
            font_scale: 1.0,
            safe_area_points: UiInsets {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            },
        },
        textures: UiTextureDelta {
            uploads: vec![],
            releases: vec![],
            full_resync: false,
        },
        primitives: vec![primitive("clipped", Some(clip)), primitive("sibling", None)],
    };
    let commands = super::ui_frame_to_scene_commands(&frame).unwrap();
    assert!(matches!(commands.as_slice(), [
        SceneCommand::PushClip { rect }, SceneCommand::Mesh2D { id, .. },
        SceneCommand::PopClip, SceneCommand::Mesh2D { id: sibling, .. },
    ] if *rect == RectI::new(10, 20, 30, 40) && id == "clipped" && sibling == "sibling"));
    // Coordinates remain logical; the shared Canvas transform applies once at presentation.
}

#[test]
fn clip_conversion_keeps_empty_bounds_and_rejects_unrepresentable_coordinates() {
    use astra_ui_core::{UiPoint, UiRect};
    let clip = |left, right| UiRect {
        min: UiPoint { x: left, y: 0.5 },
        max: UiPoint { x: right, y: 0.5 },
    };
    let empty = super::scene_clip(clip(0.5, 0.5)).unwrap();
    assert_eq!((empty.width, empty.height), (0, 0));
    let fractional = super::scene_clip(clip(-0.5, 4.25)).unwrap();
    assert_eq!((fractional.x, fractional.width), (-1, 6));
    assert!(super::scene_clip(clip(-f32::MAX, f32::MAX)).is_err());
}
