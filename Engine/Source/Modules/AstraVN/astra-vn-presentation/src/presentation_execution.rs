use astra_core::{Diagnostic, Hash256};
use astra_media_core::{
    BlendMode, CpuFilterExecutor, CpuRendererProvider, DrawCommand, FilterExecutionReport,
    FilterGraph, MediaError, RectI, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest,
    TextureFrame, Transform2D,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{LayerBlend, LayerKind, LayerState, StageModel, TextWindowState, VnError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VnPresentationExecutionRequest {
    pub stage: StageModel,
    pub assets: Vec<VnPresentationAsset>,
    pub filters: Option<FilterGraph>,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPresentationAsset {
    pub asset_id: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub hash: Hash256,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VnPresentationExecutionReport {
    pub schema: String,
    pub renderer_provider: String,
    pub filter_provider: String,
    pub input_hash: Hash256,
    pub output_hash: Hash256,
    pub draw_count: usize,
    pub filter_count: usize,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default)]
pub struct VnHeadlessPresentationExecutor;

impl VnHeadlessPresentationExecutor {
    pub fn execute(
        &self,
        request: VnPresentationExecutionRequest,
    ) -> Result<VnPresentationExecutionReport, VnError> {
        validate_stage(&request.stage)?;
        let renderer_provider = CpuRendererProvider;
        let renderer_descriptor = renderer_provider.descriptor();
        let mut renderer = renderer_provider
            .create(RendererCreateRequest {
                width: request.stage.viewport_width,
                height: request.stage.viewport_height,
                format: RenderTargetFormat::Rgba8Srgb,
                profile: request.profile,
            })
            .map_err(media_error_to_vn_error)?;

        let draw_commands = stage_draw_commands(&request.stage, &request.assets)?;
        let input_frame = renderer
            .capture_frame(&draw_commands)
            .map_err(media_error_to_vn_error)?;
        let input_hash = input_frame.hash;

        let (output_hash, filter_count, diagnostics) = if let Some(filters) = request.filters {
            let (_, filter_report) = CpuFilterExecutor
                .execute(&filters, input_frame)
                .map_err(media_error_to_vn_error)?;
            filter_result(filter_report)
        } else {
            (input_hash, 0, Vec::new())
        };

        Ok(VnPresentationExecutionReport {
            schema: "astra.vn.presentation_execution_report.v1".to_string(),
            renderer_provider: renderer_descriptor.provider_id,
            filter_provider: "astra.media.cpu_filter_executor".to_string(),
            input_hash,
            output_hash,
            draw_count: draw_commands.len(),
            filter_count,
            diagnostics,
        })
    }
}

fn filter_result(report: FilterExecutionReport) -> (Hash256, usize, Vec<Diagnostic>) {
    (
        report.output_hash,
        report.executed_nodes.len(),
        report.diagnostics,
    )
}

fn validate_stage(stage: &StageModel) -> Result<(), VnError> {
    if stage.schema != "astra.vn.stage_model.v1" {
        return Err(VnError::diagnostic(
            "ASTRA_VN_STAGE_SCHEMA",
            "stage model schema is invalid",
        ));
    }
    if stage.viewport_width == 0 || stage.viewport_height == 0 {
        return Err(VnError::diagnostic(
            "ASTRA_VN_STAGE_VIEWPORT",
            "stage viewport must be non-empty",
        ));
    }
    if stage.safe_area.left + stage.safe_area.right >= stage.viewport_width
        || stage.safe_area.top + stage.safe_area.bottom >= stage.viewport_height
    {
        return Err(VnError::diagnostic(
            "ASTRA_VN_STAGE_SAFE_AREA",
            "stage safe area must leave a non-empty drawable region",
        ));
    }
    if stage.frame_budget.max_draw_commands == 0
        || stage.frame_budget.max_filter_nodes == 0
        || stage.frame_budget.max_frame_time_us == 0
    {
        return Err(VnError::diagnostic(
            "ASTRA_VN_STAGE_FRAME_BUDGET",
            "stage frame budget limits must be non-zero",
        ));
    }
    Ok(())
}

fn stage_draw_commands(
    stage: &StageModel,
    assets: &[VnPresentationAsset],
) -> Result<Vec<DrawCommand>, VnError> {
    validate_assets(assets)?;
    let mut commands = vec![
        DrawCommand::clear([0, 0, 0, 255]),
        DrawCommand::SetCamera {
            transform: camera_transform(stage),
        },
    ];
    let mut layers = stage
        .layers
        .iter()
        .filter(|layer| layer.visible)
        .collect::<Vec<_>>();
    layers.sort_by(|left, right| left.z.cmp(&right.z).then(left.id.cmp(&right.id)));
    for layer in layers {
        commands.extend(layer_draw_commands(stage, layer, assets)?);
    }

    let mut text_windows = stage
        .text_windows
        .iter()
        .filter(|window| window.visible)
        .collect::<Vec<_>>();
    text_windows.sort_by(|left, right| left.id.cmp(&right.id));
    for window in text_windows {
        commands.push(text_window_draw_command(stage, window));
    }
    if commands.len() > stage.frame_budget.max_draw_commands as usize {
        return Err(VnError::diagnostic(
            "ASTRA_VN_STAGE_DRAW_BUDGET",
            "scene command count exceeds the declared frame budget",
        ));
    }
    Ok(commands)
}

fn layer_draw_commands(
    stage: &StageModel,
    layer: &LayerState,
    assets: &[VnPresentationAsset],
) -> Result<Vec<DrawCommand>, VnError> {
    let asset_id = layer.asset.as_deref().ok_or_else(|| {
        VnError::diagnostic(
            "ASTRA_VN_PRESENTATION_ASSET_MISSING",
            format!("visible layer {} has no cooked asset reference", layer.id),
        )
    })?;
    let asset = assets
        .iter()
        .find(|asset| asset.asset_id == asset_id)
        .ok_or_else(|| {
            VnError::diagnostic(
                "ASTRA_VN_PRESENTATION_ASSET_MISSING",
                format!("cooked presentation asset {asset_id} is missing"),
            )
        })?;
    let (x, y, width, height) = layer_rect(stage, layer);
    let frame = TextureFrame {
        width: asset.width,
        height: asset.height,
        rgba8: asset.bytes.clone(),
        hash: asset.hash,
    };
    let destination = RectI::new(x as i32, y as i32, width, height);
    let resource_id = format!("layer:{}", layer.id);
    let mut commands = vec![DrawCommand::UploadTexture {
        resource_id: resource_id.clone(),
        frame,
    }];
    if let Some(clip) = layer.clip {
        commands.push(DrawCommand::PushClip {
            rect: RectI::new(clip.x, clip.y, clip.width, clip.height),
        });
    }
    let radians = layer.transform.rotation_degrees.to_radians();
    commands.push(DrawCommand::PushTransform {
        transform: Transform2D {
            m11: radians.cos() * layer.transform.scale_x,
            m12: radians.sin() * layer.transform.scale_x,
            m21: -radians.sin() * layer.transform.scale_y,
            m22: radians.cos() * layer.transform.scale_y,
            tx: 0.0,
            ty: 0.0,
        },
    });
    commands.push(DrawCommand::Sprite {
        id: layer.id.clone(),
        texture_id: resource_id,
        source: None,
        destination,
        opacity: layer.opacity,
        blend: match layer.blend {
            LayerBlend::Alpha => BlendMode::Alpha,
            LayerBlend::Add => BlendMode::Add,
            LayerBlend::Multiply => BlendMode::Multiply,
        },
    });
    commands.push(DrawCommand::PopTransform);
    if layer.clip.is_some() {
        commands.push(DrawCommand::PopClip);
    }
    Ok(commands)
}

fn text_window_draw_command(stage: &StageModel, window: &TextWindowState) -> DrawCommand {
    let width = bounded_extent(window.width, stage.viewport_width);
    let height = bounded_extent(window.height, stage.viewport_height);
    DrawCommand::rect(
        window.id.clone(),
        bounded_coord(window.x, stage.viewport_width),
        bounded_coord(window.y, stage.viewport_height),
        width,
        height,
        [30, 30, 40, 230],
    )
}

fn layer_rect(stage: &StageModel, layer: &LayerState) -> (u32, u32, u32, u32) {
    match layer.kind {
        LayerKind::Background | LayerKind::Cg | LayerKind::Movie => {
            (0, 0, stage.viewport_width, stage.viewport_height)
        }
        LayerKind::Character => {
            let width = stage.viewport_width.clamp(1, 96);
            let height = stage.viewport_height.clamp(1, 160);
            (
                camera_coord(
                    layer.x,
                    stage.camera.x,
                    stage.camera.zoom,
                    stage.viewport_width,
                ),
                camera_coord(
                    layer.y,
                    stage.camera.y,
                    stage.camera.zoom,
                    stage.viewport_height,
                ),
                width,
                height,
            )
        }
        LayerKind::Ui | LayerKind::TextWindow | LayerKind::Effect => (
            bounded_coord(layer.x, stage.viewport_width),
            bounded_coord(layer.y, stage.viewport_height),
            stage.viewport_width.clamp(1, 64),
            stage.viewport_height.clamp(1, 32),
        ),
    }
}

fn validate_assets(assets: &[VnPresentationAsset]) -> Result<(), VnError> {
    let mut ids = std::collections::BTreeSet::new();
    for asset in assets {
        if !ids.insert(&asset.asset_id) {
            return Err(VnError::diagnostic(
                "ASTRA_VN_PRESENTATION_ASSET_DUPLICATE",
                format!("presentation asset {} is duplicated", asset.asset_id),
            ));
        }
        if asset.format != "rgba8_srgb" || asset.width == 0 || asset.height == 0 {
            return Err(VnError::diagnostic(
                "ASTRA_VN_PRESENTATION_ASSET_FORMAT",
                format!(
                    "presentation asset {} has an invalid cooked format",
                    asset.asset_id
                ),
            ));
        }
        let expected = asset.width as usize * asset.height as usize * 4;
        if asset.bytes.len() != expected {
            return Err(VnError::diagnostic(
                "ASTRA_VN_PRESENTATION_ASSET_SIZE",
                format!(
                    "presentation asset {} byte size does not match dimensions",
                    asset.asset_id
                ),
            ));
        }
        if Hash256::from_sha256(&asset.bytes) != asset.hash {
            return Err(VnError::diagnostic(
                "ASTRA_VN_PRESENTATION_ASSET_HASH",
                format!(
                    "presentation asset {} hash does not match payload",
                    asset.asset_id
                ),
            ));
        }
    }
    Ok(())
}

fn camera_transform(stage: &StageModel) -> Transform2D {
    let radians = stage.camera.rotation.to_radians();
    let cosine = radians.cos() * stage.camera.zoom;
    let sine = radians.sin() * stage.camera.zoom;
    Transform2D {
        m11: cosine,
        m12: sine,
        m21: -sine,
        m22: cosine,
        tx: -stage.camera.x,
        ty: -stage.camera.y,
    }
}

fn camera_coord(value: f32, camera: f32, zoom: f32, limit: u32) -> u32 {
    bounded_coord((value - camera) * zoom.max(0.01), limit)
}

fn bounded_coord(value: f32, limit: u32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    value.round().min(limit as f32) as u32
}

fn bounded_extent(value: f32, limit: u32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 1;
    }
    value.round().clamp(1.0, limit.max(1) as f32) as u32
}

fn media_error_to_vn_error(error: MediaError) -> VnError {
    match error {
        MediaError::Diagnostics(diagnostics) => diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == astra_core::DiagnosticSeverity::Blocking)
            .or_else(|| diagnostics.first())
            .cloned()
            .map(VnError::Diagnostic)
            .unwrap_or_else(|| VnError::message("media diagnostics blocked")),
        MediaError::Message(message) => VnError::message(message),
    }
}
