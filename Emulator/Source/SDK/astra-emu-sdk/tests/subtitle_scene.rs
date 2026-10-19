use astra_emu_sdk::{
    subtitle::{FontData, VideoColorSpace},
    AssScene,
};
use astra_media_core::{RectI, SceneCommand};
use std::sync::Arc;

fn fonts() -> Vec<FontData> {
    vec![FontData {
        name: "public.ttf".into(),
        bytes: Arc::from(
            include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf")
                .as_slice(),
        ),
    }]
}
fn ass(text: &str) -> String {
    format!("[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nYCbCr Matrix: None\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Noto Sans JP Thin,24,&H000000FF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}\n")
}
#[test]
fn borrowed_masks_are_retained_for_pause_and_position_only_motion() {
    let text = ass("{\\move(20,30,120,30)}Public");
    let viewport = RectI {
        x: 17,
        y: 29,
        width: 640,
        height: 360,
    };
    let mut scene = AssScene::new("public.ass".into(), text.as_bytes(), &fonts()).unwrap();
    let mut mutations = Vec::new();
    assert!(scene
        .update(
            0,
            (640, 360),
            viewport,
            VideoColorSpace::Rgb,
            &mut mutations
        )
        .unwrap());
    assert!(mutations
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
    assert!(!mutations
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadTexture { .. })));
    let mut first = Vec::new();
    scene.append_draws(&mut first);
    assert!(first.iter().all(|command| matches!(
        command,
        SceneCommand::GlyphRun {
            rgba: [255, 0, 0, 255],
            ..
        }
    )));
    mutations.clear();
    assert!(!scene
        .update(
            0,
            (640, 360),
            viewport,
            VideoColorSpace::Rgb,
            &mut mutations
        )
        .unwrap());
    assert!(mutations.is_empty());
    assert!(scene
        .update(
            1_000_000,
            (640, 360),
            viewport,
            VideoColorSpace::Rgb,
            &mut mutations
        )
        .unwrap());
    assert!(
        mutations.is_empty(),
        "position-only changes must not upload or release masks"
    );
    let mut moved = Vec::new();
    scene.append_draws(&mut moved);
    assert_ne!(first, moved);
    assert!(scene
        .update(
            3_000_000,
            (640, 360),
            viewport,
            VideoColorSpace::Rgb,
            &mut mutations
        )
        .unwrap());
    assert!(mutations
        .iter()
        .all(|command| matches!(command, SceneCommand::ReleaseResource { .. })));
    moved.clear();
    scene.append_draws(&mut moved);
    assert!(moved.is_empty());
    mutations.clear();
    assert!(scene
        .update(
            0,
            (640, 360),
            viewport,
            VideoColorSpace::Rgb,
            &mut mutations
        )
        .unwrap());
    assert!(mutations
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
    mutations.clear();
    scene.release_resources(&mut mutations);
    assert!(!mutations.is_empty());
    moved.clear();
    scene.append_draws(&mut moved);
    assert!(moved.is_empty());
}

#[test]
fn viewport_origin_and_storage_are_separate_from_display_raster() {
    let text = ass("{\\pos(20,30)}Public");
    let mut a = AssScene::new("a".into(), text.as_bytes(), &fonts()).unwrap();
    let mut b = AssScene::new("b".into(), text.as_bytes(), &fonts()).unwrap();
    let mut mutations = Vec::new();
    a.update(
        0,
        (1280, 720),
        RectI {
            x: 0,
            y: 0,
            width: 640,
            height: 360,
        },
        VideoColorSpace::Rgb,
        &mut mutations,
    )
    .unwrap();
    b.update(
        0,
        (1280, 720),
        RectI {
            x: 17,
            y: 29,
            width: 640,
            height: 360,
        },
        VideoColorSpace::Rgb,
        &mut mutations,
    )
    .unwrap();
    let mut left = Vec::new();
    let mut right = Vec::new();
    a.append_draws(&mut left);
    b.append_draws(&mut right);
    assert!(!left.is_empty());
    for (a, b) in left.iter().zip(&right) {
        let (SceneCommand::GlyphRun { glyphs: a, .. }, SceneCommand::GlyphRun { glyphs: b, .. }) =
            (a, b)
        else {
            panic!()
        };
        assert_eq!(b[0].x - a[0].x, 17);
        assert_eq!(b[0].y - a[0].y, 29);
    }
}

#[test]
fn dropping_and_reopening_renderer_starts_with_new_mask_residency() {
    let text = ass("Public");
    for _ in 0..3 {
        let mut scene = AssScene::new("generation".into(), text.as_bytes(), &fonts()).unwrap();
        let mut commands = Vec::new();
        scene
            .update(
                1_000_000,
                (640, 360),
                RectI {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 360,
                },
                VideoColorSpace::Rgb,
                &mut commands,
            )
            .unwrap();
        assert!(commands
            .iter()
            .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
        scene.release_resources(&mut commands);
    }
}
