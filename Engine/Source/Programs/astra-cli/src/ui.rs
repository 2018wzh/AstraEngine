use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use astra_cook::CookCancellationToken;
use astra_core::Hash256;
use astra_media::{
    frame_hash, CpuRendererProvider, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest,
};
use astra_package::PackageReader;
use astra_player_core::{evidence_scene_command_hash, PlayerHostCommand, PlayerHostResourceId};
use astra_player_vn::NativeVnHostCommandSource;
use astra_vn_core::{VnRunConfig, VnWaitKind};
use astra_vn_script::AstraSource;
use serde::Serialize;
use tempfile::NamedTempFile;

use super::*;

#[derive(Serialize)]
pub(crate) struct UiSnapshotReport {
    pub(crate) schema: String,
    pub(crate) profile: String,
    pub(crate) target: String,
    pub(crate) locale: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale_factor: f32,
    pub(crate) frame_hash: String,
    pub(crate) semantic_hash: String,
    pub(crate) scene_hash: String,
    pub(crate) performance: astra_ui_core::UiPerformanceReport,
}

pub(crate) struct UiSnapshotOutput {
    pub(crate) rgba8: Vec<u8>,
    pub(crate) semantics: astra_ui_core::UiSemanticSnapshot,
    pub(crate) commands: Vec<astra_media::SceneCommand>,
    pub(crate) report: UiSnapshotReport,
}

#[derive(serde::Serialize)]
pub(crate) struct UiSceneEvidence {
    pub(crate) schema: &'static str,
    pub(crate) scene_hash: String,
    pub(crate) command_count: u32,
    pub(crate) command_counts: BTreeMap<&'static str, u32>,
}

pub(crate) fn run_ui_command(command: UiCommand) -> Result<(), CliError> {
    match command {
        UiCommand::Check {
            project,
            profile,
            target,
        } => {
            let package = compile_ui_preview_package(&project, &profile, &target)?;
            println!(
                "{}",
                serde_json::json!({
                    "schema": "astra.ui_check_report.v1",
                    "status": "pass",
                    "profile": profile,
                    "target": target,
                    "package_hash": Hash256::from_sha256(&package).to_string(),
                })
            );
        }
        UiCommand::Preview {
            project,
            profile,
            target,
            locale,
            width,
            height,
            out,
        } => {
            let package = compile_ui_preview_package(&project, &profile, &target)?;
            let snapshot =
                render_ui_snapshot(&package, &profile, &target, &locale, width, height, 1.0)?;
            write_png(&out, width, height, &snapshot.rgba8)?;
            println!("{}", serde_json::to_string(&snapshot.report)?);
        }
        UiCommand::Snapshot {
            project,
            profile,
            target,
            locale,
            width,
            height,
            out,
        } => {
            let package = compile_ui_preview_package(&project, &profile, &target)?;
            let snapshot =
                render_ui_snapshot(&package, &profile, &target, &locale, width, height, 1.0)?;
            write_ui_snapshot(&out, &snapshot)?;
            println!("{}", serde_json::to_string(&snapshot.report)?);
        }
        UiCommand::Matrix {
            project,
            profile,
            target,
            locales,
            sizes,
            scales,
            out,
        } => {
            if locales.is_empty() || sizes.is_empty() || scales.is_empty() {
                return Err("ASTRA_UI_MATRIX_EMPTY: locales, sizes and scales are required".into());
            }
            let package = compile_ui_preview_package(&project, &profile, &target)?;
            fs::create_dir_all(&out)?;
            let mut reports = Vec::new();
            for locale in locales {
                for size in &sizes {
                    let (width, height) = parse_ui_matrix_size(size)?;
                    for scale in &scales {
                        if !scale.is_finite() || !(0.5..=4.0).contains(scale) {
                            return Err(
                                "ASTRA_UI_MATRIX_SCALE: scale must be within 0.5..=4.0".into()
                            );
                        }
                        let snapshot = render_ui_snapshot(
                            &package, &profile, &target, &locale, width, height, *scale,
                        )?;
                        let id = format!(
                            "{locale}-{width}x{height}-{}",
                            scale.to_string().replace('.', "_")
                        );
                        write_ui_snapshot(&out.join(&id), &snapshot)?;
                        reports.push(snapshot.report);
                    }
                }
            }
            let matrix = serde_json::json!({
                "schema": "astra.ui_matrix_report.v1",
                "profile": profile,
                "target": target,
                "package_hash": Hash256::from_sha256(&package).to_string(),
                "entries": reports,
            });
            atomic_replace(
                &out.join("matrix.report.json"),
                &serde_json::to_vec_pretty(&matrix)?,
            )?;
            println!("{}", serde_json::to_string(&matrix)?);
        }
    }
    Ok(())
}

pub(crate) fn compile_ui_preview_package(
    project: &Path,
    profile: &str,
    target: &str,
) -> Result<Vec<u8>, CliError> {
    let temp = tempfile::tempdir()?;
    let cooked = temp.path().join("cooked");
    let manifest = cook_project(
        project.to_path_buf(),
        profile,
        Some(target),
        cooked.clone(),
        &CookCancellationToken::default(),
    )?;
    Ok(build_package_from_cooked(&cooked, manifest, Some(target), None)?.into_bytes())
}

pub(crate) fn render_ui_snapshot(
    package_bytes: &[u8],
    profile: &str,
    target: &str,
    locale: &str,
    width: u32,
    height: u32,
    scale_factor: f32,
) -> Result<UiSnapshotOutput, CliError> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("ASTRA_UI_PREVIEW_VIEWPORT: dimensions must be within 1..=8192".into());
    }
    let package = PackageReader::open(package_bytes)?;
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig {
            profile: profile.to_string(),
            locale: locale.to_string(),
        },
        width,
        height,
        PlayerHostResourceId(1),
    )?;
    let mut batches = vec![source.launch()?];
    if scale_factor != 1.0 {
        batches.push(
            source.dispatch_ui_event(astra_ui_core::UiInputEventKind::Resize {
                viewport: astra_ui_core::UiViewport {
                    physical_width: width,
                    physical_height: height,
                    scale_factor,
                    safe_area_points: astra_ui_core::UiInsets {
                        left: 0.0,
                        top: 0.0,
                        right: 0.0,
                        bottom: 0.0,
                    },
                    font_scale: 1.0,
                },
            })?,
        );
    }
    batches.reverse();
    let mut renderer = CpuRendererProvider.create(RendererCreateRequest {
        width,
        height,
        format: RenderTargetFormat::Rgba8Srgb,
        profile: "ui-preview".into(),
    })?;
    let mut selected = None;
    for attempt in 0..64_u64 {
        let batch = if let Some(batch) = batches.pop() {
            batch
        } else if let Some(wait) = source.pending_wait().cloned() {
            match wait.kind {
                VnWaitKind::Dialogue | VnWaitKind::Choice | VnWaitKind::SystemPage => source
                    .dispatch_ui_event(astra_ui_core::UiInputEventKind::Keyboard {
                        logical_key: "Enter".into(),
                        physical_key: "Enter".into(),
                        state: astra_ui_core::UiButtonState::Pressed,
                        repeat: false,
                        modifiers: 0,
                    })?,
                _ => source.complete_wait(wait.fence)?,
            }
        } else {
            source.dispatch_ui_event(astra_ui_core::UiInputEventKind::Keyboard {
                logical_key: "Enter".into(),
                physical_key: "Enter".into(),
                state: astra_ui_core::UiButtonState::Pressed,
                repeat: false,
                modifiers: 0,
            })?
        };
        for command in batch.commands {
            let PlayerHostCommand::PresentScene {
                sequence,
                clear_rgba,
                commands,
                semantics,
                ..
            } = command
            else {
                continue;
            };
            let mut scene = Vec::with_capacity(commands.len() + 1);
            scene.push(astra_media::SceneCommand::Clear { rgba: clear_rgba });
            scene.extend(commands.iter().cloned());
            let frame = renderer.capture_frame(&scene)?;
            if let Some(semantics) = semantics {
                selected = Some((frame, commands, semantics, sequence));
                break;
            }
        }
        if selected.is_some() {
            break;
        }
        if attempt == 63 {
            return Err(
                "ASTRA_UI_PREVIEW_SEMANTICS: no UI surface became active within 64 physical inputs"
                    .into(),
            );
        }
    }
    let (mut frame, mut commands, mut semantics, mut sequence) = selected
        .ok_or("ASTRA_UI_PREVIEW_PRESENT_SCENE: launch did not produce a semantic Scene2D frame")?;
    for tick in 1..=30_u64 {
        let batch = source.dispatch_ui_event(astra_ui_core::UiInputEventKind::FixedTime {
            time_ns: tick * 16_666_667,
        })?;
        for command in batch.commands {
            let PlayerHostCommand::PresentScene {
                sequence: next_sequence,
                clear_rgba,
                commands: next_commands,
                semantics: next_semantics,
                ..
            } = command
            else {
                continue;
            };
            let mut scene = Vec::with_capacity(next_commands.len() + 1);
            scene.push(astra_media::SceneCommand::Clear { rgba: clear_rgba });
            scene.extend(next_commands.iter().cloned());
            frame = renderer.capture_frame(&scene)?;
            commands = next_commands;
            sequence = next_sequence;
            if let Some(next_semantics) = next_semantics {
                semantics = next_semantics;
            }
        }
    }
    let scene_hash = evidence_scene_command_hash(sequence, width, height, &commands);
    let rendered_hash = frame_hash(frame.width, frame.height, frame.format, &frame.bytes);
    Ok(UiSnapshotOutput {
        rgba8: frame.bytes,
        semantics: semantics.clone(),
        commands,
        report: UiSnapshotReport {
            schema: "astra.ui_snapshot_report.v1".into(),
            profile: profile.into(),
            target: target.into(),
            locale: locale.into(),
            width,
            height,
            scale_factor,
            frame_hash: rendered_hash.to_string(),
            semantic_hash: format!("generation:{}", semantics.generation),
            scene_hash: scene_hash.to_string(),
            performance: source.ui_performance_report(),
        },
    })
}

pub(crate) fn write_ui_snapshot(out: &Path, snapshot: &UiSnapshotOutput) -> Result<(), CliError> {
    fs::create_dir_all(out)?;
    write_png(
        &out.join("frame.png"),
        snapshot.report.width,
        snapshot.report.height,
        &snapshot.rgba8,
    )?;
    atomic_replace(
        &out.join("semantic.json"),
        &serde_json::to_vec_pretty(&snapshot.semantics)?,
    )?;
    atomic_replace(
        &out.join("scene.json"),
        &serde_json::to_vec_pretty(&ui_scene_evidence(
            &snapshot.commands,
            &snapshot.report.scene_hash,
        )?)?,
    )?;
    atomic_replace(
        &out.join("report.json"),
        &serde_json::to_vec_pretty(&snapshot.report)?,
    )?;
    Ok(())
}

pub(crate) fn ui_scene_evidence(
    commands: &[astra_media::SceneCommand],
    scene_hash: &str,
) -> Result<UiSceneEvidence, CliError> {
    let command_count = u32::try_from(commands.len())
        .map_err(|_| "ASTRA_UI_SNAPSHOT_COMMAND_LIMIT: scene command count exceeds u32")?;
    let mut command_counts = BTreeMap::new();
    for command in commands {
        let count = command_counts
            .entry(scene_command_kind(command))
            .or_insert(0_u32);
        *count = count
            .checked_add(1)
            .ok_or("ASTRA_UI_SNAPSHOT_COMMAND_LIMIT: command kind count overflow")?;
    }
    Ok(UiSceneEvidence {
        schema: "astra.ui_scene_evidence.v1",
        scene_hash: scene_hash.to_string(),
        command_count,
        command_counts,
    })
}

pub(crate) fn scene_command_kind(command: &astra_media::SceneCommand) -> &'static str {
    use astra_media::SceneCommand;
    match command {
        SceneCommand::UploadTexture { .. } => "upload_texture",
        SceneCommand::UpdateTextureRegion { .. } => "update_texture_region",
        SceneCommand::UploadGlyph { .. } => "upload_glyph",
        SceneCommand::ReleaseResource { .. } => "release_resource",
        SceneCommand::Sprite { .. } => "sprite",
        SceneCommand::GlyphRun { .. } => "glyph_run",
        SceneCommand::Mesh2D { .. } => "mesh2d",
        SceneCommand::MeshBatch2D { .. } => "mesh_batch2d",
        SceneCommand::Clear { .. } => "clear",
        SceneCommand::Rect { .. } => "rect",
        SceneCommand::Texture { .. } => "texture",
        SceneCommand::VideoFrame { .. } => "video_frame",
        SceneCommand::Glyph { .. } => "glyph",
        SceneCommand::PushClip { .. } => "push_clip",
        SceneCommand::PopClip => "pop_clip",
        SceneCommand::PushTransform { .. } => "push_transform",
        SceneCommand::PopTransform => "pop_transform",
        SceneCommand::SetCamera { .. } => "set_camera",
        SceneCommand::PushOpacity { .. } => "push_opacity",
        SceneCommand::PopOpacity => "pop_opacity",
        SceneCommand::PushPixelMask { .. } => "push_pixel_mask",
        SceneCommand::PopPixelMask => "pop_pixel_mask",
        SceneCommand::FilterGraph { .. } => "filter_graph",
    }
}

pub(crate) fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    rgba8: &[u8],
) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    image::save_buffer_with_format(
        path,
        rgba8,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    Ok(())
}

pub(crate) fn parse_ui_matrix_size(value: &str) -> Result<(u32, u32), CliError> {
    let (width, height) = value
        .split_once('x')
        .ok_or("ASTRA_UI_MATRIX_SIZE: size must use WIDTHxHEIGHT")?;
    let width = width.parse::<u32>()?;
    let height = height.parse::<u32>()?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("ASTRA_UI_MATRIX_SIZE: dimensions must be within 1..=8192".into());
    }
    Ok((width, height))
}

pub(crate) fn read_astra_sources(paths: &[PathBuf]) -> Result<Vec<AstraSource>, CliError> {
    paths
        .iter()
        .map(|path| {
            if path.extension().and_then(|extension| extension.to_str()) != Some("astra") {
                return Err(format!(
                    "ASTRA_SCRIPT_SOURCE_EXTENSION: source must use the .astra extension: {}",
                    path.display()
                )
                .into());
            }
            Ok(AstraSource::story(
                path.to_string_lossy(),
                fs::read_to_string(path)?,
            ))
        })
        .collect()
}

pub(crate) fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "ASTRA_SCRIPT_FORMAT_PATH: source has no parent: {}",
            path.display()
        )
    })?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file_mut().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}
