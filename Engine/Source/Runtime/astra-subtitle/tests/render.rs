use astra_subtitle::{
    AssRenderer, ErrorKind, FontData, FrameChange, SubtitleFrame, VideoColorSpace, YcbcrMatrix,
};
use std::sync::{Arc, LazyLock};

const ASS: &str = include_str!("fixtures/timeline.ass");
const SIZE: (u32, u32) = (320, 180);

fn fonts() -> [FontData; 1] {
    // Existing public OFL-1.1 fixture; provenance/license live next to the font
    // in Engine/Fixtures/PublicDomainFonts (not a commercial game attachment).
    static FONT: LazyLock<Arc<[u8]>> = LazyLock::new(|| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../Fixtures/PublicDomainFonts/NotoSansSC-Variable.ttf");
        Arc::from(std::fs::read(path).expect("repository OFL font fixture"))
    });
    [FontData {
        name: "NotoSansSC-Variable.ttf".into(),
        bytes: FONT.clone(),
    }]
}

type SnapshotRun = (u32, u32, u32, u32, usize, u32, Vec<u8>);

fn snapshot(frame: &SubtitleFrame<'_>) -> Vec<SnapshotRun> {
    frame
        .runs
        .iter()
        .map(|run| {
            assert_eq!(
                run.alpha.len(),
                run.stride * (run.h as usize - 1) + run.w as usize
            );
            let pixels: Vec<_> = (0..run.h as usize)
                .flat_map(|row| {
                    run.alpha[row * run.stride..row * run.stride + run.w as usize]
                        .iter()
                        .copied()
                })
                .collect();
            (run.x, run.y, run.w, run.h, run.stride, run.color, pixels)
        })
        .collect()
}

fn bounds(frame: &SubtitleFrame<'_>) -> (u32, u32, u32, u32) {
    let mut bounds = (u32::MAX, u32::MAX, 0, 0);
    for run in frame.runs.iter() {
        for row in 0..run.h as usize {
            for column in 0..run.w as usize {
                if run.pixel_alpha(run.alpha[row * run.stride + column]) == 0 {
                    continue;
                }
                let x = run.x + column as u32;
                let y = run.y + row as u32;
                bounds = (
                    bounds.0.min(x),
                    bounds.1.min(y),
                    bounds.2.max(x + 1),
                    bounds.3.max(y + 1),
                );
            }
        }
    }
    bounds
}

#[test]
fn real_public_font_timeline_pause_seek_resize_and_replacement() {
    let mut renderer = AssRenderer::new(ASS.as_bytes(), &fonts()).unwrap();
    let initial = {
        let frame = renderer.render(0, SIZE, SIZE).unwrap();
        assert!(frame.changed);
        assert_eq!(frame.change, FrameChange::Content);
        assert_eq!(frame.matrix, YcbcrMatrix::None);
        assert!(!frame.runs.is_empty());
        let (x, y, right, bottom) = bounds(&frame);
        assert!(x >= 20 && y >= 40 && right < 100 && bottom < 100);
        snapshot(&frame)
    };
    {
        let paused = renderer.render(0, SIZE, SIZE).unwrap();
        assert!(!paused.changed);
        assert_eq!(paused.change, FrameChange::Unchanged);
        assert_eq!(snapshot(&paused), initial);
    }
    let rectangle = {
        let frame = renderer.render(1000, SIZE, SIZE).unwrap();
        assert!(frame.changed);
        assert!(!frame.runs.is_empty());
        let (x, y, right, bottom) = bounds(&frame);
        assert!(x.abs_diff(80) <= 2 && y.abs_diff(40) <= 2);
        assert!(right.abs_diff(110) <= 2 && bottom.abs_diff(60) <= 2);
        let mut opaque_mask_pixel = false;
        for run in frame.runs.iter() {
            assert_eq!(run.color, 0xff000080);
            assert_eq!(run.opacity(), 127);
            assert_eq!(
                run.rgba_for_video(frame.matrix, VideoColorSpace::Bt709Tv)
                    .unwrap(),
                [255, 0, 0, 127]
            );
            for row in 0..run.h as usize {
                for &coverage in &run.alpha[row * run.stride..row * run.stride + run.w as usize] {
                    opaque_mask_pixel |= coverage == 255 && run.pixel_alpha(coverage) == 127;
                }
            }
        }
        assert!(opaque_mask_pixel);
        snapshot(&frame)
    };
    {
        let paused = renderer.render(1000, SIZE, SIZE).unwrap();
        assert_eq!(paused.change, FrameChange::Unchanged);
        assert_eq!(snapshot(&paused), rectangle);
    }
    assert!(renderer.render(2000, SIZE, SIZE).unwrap().runs.is_empty());
    assert_eq!(snapshot(&renderer.render(0, SIZE, SIZE).unwrap()), initial);
    {
        let resized = renderer.render(1000, SIZE, (640, 360)).unwrap();
        let (x, y, right, bottom) = bounds(&resized);
        assert!(x.abs_diff(160) <= 2 && y.abs_diff(80) <= 2);
        assert!(right.abs_diff(220) <= 2 && bottom.abs_diff(120) <= 2);
    }
    assert_eq!(
        snapshot(&renderer.render(1000, SIZE, SIZE).unwrap()),
        rectangle
    );
    // Replacement owns a fresh library/track; no old generation is composited.
    drop(renderer);
    let empty = ASS
        .lines()
        .filter(|line| !line.starts_with("Dialogue:"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut replacement = AssRenderer::new(empty.as_bytes(), &fonts()).unwrap();
    assert!(replacement.render(0, SIZE, SIZE).unwrap().runs.is_empty());
}

#[test]
fn no_system_font_or_family_fallback_and_inline_font_tags_are_checked() {
    let missing = ASS.replace("Noto Sans SC", "Astra Definitely Missing Family");
    let mut renderer = AssRenderer::new(missing.as_bytes(), &fonts()).unwrap();
    assert_eq!(
        renderer.render(0, SIZE, SIZE).err().unwrap().kind,
        ErrorKind::MissingFont
    );
    // Native failure is sticky even if a later timestamp would be empty.
    assert_eq!(
        renderer.render(2000, SIZE, SIZE).err().unwrap().kind,
        ErrorKind::MissingFont
    );
    let inline = ASS.replace("字幕", "{\\fnAstra Missing Inline Font}字幕");
    let mut renderer = AssRenderer::new(inline.as_bytes(), &fonts()).unwrap();
    assert_eq!(
        renderer.render(0, SIZE, SIZE).err().unwrap().kind,
        ErrorKind::MissingFont
    );
    let mut renderer = AssRenderer::new(ASS.as_bytes(), &[]).unwrap();
    assert_eq!(
        renderer.render(0, SIZE, SIZE).err().unwrap().kind,
        ErrorKind::MissingFont
    );
}

#[test]
fn missing_glyph_fails_instead_of_successful_tofu() {
    let missing = ASS.replace("字幕", "\u{10ffff}");
    let mut renderer = AssRenderer::new(missing.as_bytes(), &fonts()).unwrap();
    let error = renderer.render(0, SIZE, SIZE).err().unwrap();
    assert_eq!(error.kind, ErrorKind::MissingGlyph);
    let safe = error.to_string();
    assert!(!safe.contains("Noto") && !safe.contains("10ffff") && !safe.contains("字幕"));
}

#[test]
fn invalid_utf8_nul_ass_fonts_time_and_geometry_fail_safely() {
    for input in [&b"invalid ASS"[..], &b"\xff"[..], &b"\0"[..], &b""[..]] {
        assert!(AssRenderer::new(input, &fonts()).is_err());
    }
    let bad = [FontData {
        name: "invalid.ttf".into(),
        bytes: Arc::from(&b"invalid font"[..]),
    }];
    assert!(AssRenderer::new(ASS.as_bytes(), &bad).is_err());
    let nul = [FontData {
        name: "private\0name".into(),
        bytes: fonts()[0].bytes.clone(),
    }];
    assert!(AssRenderer::new(ASS.as_bytes(), &nul).is_err());
    let mut renderer = AssRenderer::new(ASS.as_bytes(), &fonts()).unwrap();
    for (time, storage, raster) in [
        (u64::MAX, SIZE, SIZE),
        (0, (0, 180), SIZE),
        (0, SIZE, (u32::MAX, 180)),
        (0, (100_000, 100_000), SIZE),
    ] {
        assert_eq!(
            renderer.render(time, storage, raster).err().unwrap().kind,
            ErrorKind::Bounds
        );
    }
    assert!(!renderer.render(0, SIZE, SIZE).unwrap().runs.is_empty());
}

#[test]
fn track_matrix_is_exposed_independently_of_render_geometry() {
    for (header, expected) in [
        ("TV.601", YcbcrMatrix::Bt601Tv),
        ("PC.709", YcbcrMatrix::Bt709Pc),
        ("None", YcbcrMatrix::None),
        ("unknown", YcbcrMatrix::Unknown),
    ] {
        let ass = ASS.replace("YCbCr Matrix: None", &format!("YCbCr Matrix: {header}"));
        let mut renderer = AssRenderer::new(ass.as_bytes(), &fonts()).unwrap();
        assert_eq!(renderer.render(1000, SIZE, SIZE).unwrap().matrix, expected);
        assert_eq!(
            renderer.render(1000, SIZE, (1920, 1080)).unwrap().matrix,
            expected
        );
    }
}

#[test]
fn integer_motion_preserves_native_position_only_change() {
    let ass = ASS.replace("\\pos(80,40)", "\\move(80,40,100,40)");
    let mut renderer = AssRenderer::new(ass.as_bytes(), &fonts()).unwrap();
    let first = snapshot(&renderer.render(1000, SIZE, SIZE).unwrap());
    let frame = renderer.render(1500, SIZE, SIZE).unwrap();
    assert_eq!(frame.change, FrameChange::Positions);
    let second = snapshot(&frame);
    assert_eq!(first.len(), second.len());
    for (a, b) in first.iter().zip(&second) {
        assert_eq!(b.0, a.0 + 10);
        assert_eq!(
            (a.1, a.2, a.3, a.4, a.5, &a.6),
            (b.1, b.2, b.3, b.4, b.5, &b.6)
        );
    }
}

#[test]
fn zero_duration_missing_font_event_never_requires_a_fallback() {
    let ass = format!("{ASS}\nDialogue: 0,0:00:00.00,0:00:00.00,Default,,0,0,0,,{{\\fnAstra Inactive Missing Family}}Inactive\n");
    let mut renderer = AssRenderer::new(ass.as_bytes(), &fonts()).unwrap();
    assert!(!renderer.render(0, SIZE, SIZE).unwrap().runs.is_empty());
    assert!(!renderer.render(1000, SIZE, SIZE).unwrap().runs.is_empty());
    assert!(renderer.render(2000, SIZE, SIZE).unwrap().runs.is_empty());
}
