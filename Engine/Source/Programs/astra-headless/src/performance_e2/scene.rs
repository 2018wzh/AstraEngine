use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::Duration,
};

use astra_core::{Hash256, PerformanceBudget};
use astra_media_core::{
    BlendMode, FilterGraph, FilterNode, FilterParam, FilterTarget, GlyphBitmap, GlyphBitmapFormat,
    GlyphInstance, MeshMaterial2D, MeshVertex2D, RectI, SceneCommand, TextureFilter2D,
    TextureFrame, Transform2D,
};
use astra_platform::SceneFrame;

use super::*;

pub(crate) fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

pub(crate) fn scene2d_workload() -> (SceneFrame, SceneFrame) {
    let texture_bytes = [64_u8, 128, 224, 255].repeat(512 * 512);
    let glyph_bytes = vec![192_u8; 64 * 64];
    let texture = TextureFrame {
        width: 512,
        height: 512,
        rgba8: texture_bytes.into(),
    };
    let glyph = GlyphBitmap {
        width: 64,
        height: 64,
        format: GlyphBitmapFormat::Alpha8,
        pixels: glyph_bytes.into(),
    };
    let mut frame = SceneFrame {
        sequence: 2,
        width: 1920,
        height: 1080,
        clear_rgba: [4, 8, 16, 255],
        commands: draw_commands(),
        semantics: None,
    };
    let mut initialization_commands = Vec::with_capacity(frame.commands.len() + 2);
    initialization_commands.push(SceneCommand::UploadTexture {
        resource_id: "performance.texture.0".into(),
        frame: texture,
    });
    initialization_commands.push(SceneCommand::UploadGlyph {
        resource_id: "performance.glyph.0".into(),
        glyph,
    });
    initialization_commands.extend(frame.commands.clone());
    let initialization = SceneFrame {
        sequence: 1,
        width: frame.width,
        height: frame.height,
        clear_rgba: frame.clear_rgba,
        commands: initialization_commands,
        semantics: None,
    };
    frame.sequence = 2;
    (initialization, frame)
}

pub(crate) fn draw_commands() -> Vec<SceneCommand> {
    let mut fade_params = BTreeMap::new();
    fade_params.insert("amount".into(), FilterParam::Float(0.92));
    vec![
        SceneCommand::SetCamera {
            transform: Transform2D::IDENTITY,
        },
        SceneCommand::Sprite {
            id: "performance.sprite.0".into(),
            texture_id: "performance.texture.0".into(),
            source: None,
            destination: RectI::new(64, 64, 1024, 768),
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
        SceneCommand::GlyphRun {
            id: "performance.glyph_run.0".into(),
            glyphs: vec![GlyphInstance {
                resource_id: "performance.glyph.0".into(),
                x: 128,
                y: 128,
                rotation_quadrants: 0,
            }]
            .into(),
            rgba: [255, 240, 220, 255],
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
        SceneCommand::Mesh2D {
            id: "performance.mesh.0".into(),
            vertices: vec![
                MeshVertex2D {
                    position: [960.0, 128.0],
                    uv: [0.0, 0.0],
                    premultiplied_rgba: [240, 80, 80, 255],
                },
                MeshVertex2D {
                    position: [1760.0, 900.0],
                    uv: [1.0, 1.0],
                    premultiplied_rgba: [80, 240, 80, 255],
                },
                MeshVertex2D {
                    position: [720.0, 900.0],
                    uv: [0.0, 1.0],
                    premultiplied_rgba: [80, 80, 240, 255],
                },
            ]
            .into(),
            indices: vec![0, 1, 2].into(),
            material: MeshMaterial2D::Solid,
            texture_id: None,
            texture_filter: TextureFilter2D::Linear,
            opacity: 0.9,
            blend: BlendMode::Alpha,
        },
        SceneCommand::PushClip {
            rect: RectI::new(32, 32, 1856, 1016),
        },
        SceneCommand::PushOpacity { opacity: 0.98 },
        SceneCommand::Rect {
            id: "performance.rect.0".into(),
            x: 1400,
            y: 120,
            width: 320,
            height: 180,
            rgba: [32, 48, 80, 220],
        },
        SceneCommand::PopOpacity,
        SceneCommand::PopClip,
        SceneCommand::FilterGraph {
            graph: FilterGraph {
                schema: "astra.filter_graph.v1".into(),
                nodes: vec![FilterNode {
                    id: "performance.fade.0".into(),
                    kind: "astra.filter.fade".into(),
                    input: FilterTarget::Final,
                    output: FilterTarget::Final,
                    params: fade_params,
                    deterministic: true,
                    allow_cpu_fallback: false,
                }],
            },
        },
    ]
}

pub(crate) fn animate(frame: &mut SceneFrame, sequence: u64) -> Result<(), String> {
    frame.sequence = sequence + 1;
    if !frame
        .commands
        .iter()
        .any(|command| matches!(command, SceneCommand::SetCamera { .. }))
    {
        return Err("ASTRA_PERFORMANCE_CAMERA_COMMAND_MISSING".into());
    }
    // Keep the texture, glyph, mesh, clip, transform, opacity, and FilterGraph geometry stable
    // so this workload proves that persistent GPU resources are not streamed again. The clear
    // color pulse still changes every submitted frame and therefore prevents a static-frame
    // elision from turning the 120 Hz renderer workload into a no-op.
    frame.clear_rgba[0] = 4 + (sequence % 16) as u8;
    Ok(())
}

pub(crate) fn validate_metric_set(budget: &PerformanceBudget) -> Result<(), String> {
    let actual = budget
        .metrics
        .iter()
        .map(|metric| metric.id.as_str())
        .collect::<BTreeSet<_>>();
    let required = REQUIRED_METRICS.iter().copied().collect::<BTreeSet<_>>();
    if actual != required
        || budget.metrics.iter().any(|metric| {
            let expected = if metric.id.starts_with("profiler.") {
                1
            } else {
                MEASURED_FRAMES as usize
            };
            metric.min_samples != expected || metric.max_samples != expected
        })
    {
        return Err("ASTRA_PERFORMANCE_BUDGET_METRIC_SET_INVALID".into());
    }
    Ok(())
}

pub(crate) fn duration_ns(duration: Duration) -> Result<u64, String> {
    duration
        .as_nanos()
        .try_into()
        .map_err(|_| "ASTRA_PERFORMANCE_DURATION_OVERFLOW".into())
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    role: &str,
) -> Result<T, String> {
    serde_json::from_slice(
        &fs::read(path)
            .map_err(|error| format!("ASTRA_PERFORMANCE_{role}_READ_FAILED: {error}"))?,
    )
    .map_err(|error| format!("ASTRA_PERFORMANCE_{role}_INVALID: {error}"))
}

pub(crate) fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "ASTRA_PERFORMANCE_OUTPUT_PATH_INVALID".to_string())?;
    let temporary = path.with_file_name(format!(".{name}.partial-{}", std::process::id()));
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

pub(crate) fn hash_file(path: &Path) -> Result<String, String> {
    Ok(Hash256::from_sha256_file(path)
        .map_err(|error| error.to_string())?
        .to_string())
}
