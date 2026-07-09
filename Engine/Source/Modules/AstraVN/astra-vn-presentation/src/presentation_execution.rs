use astra_core::{Diagnostic, Hash256};
use astra_media_core::{
    CpuFilterExecutor, DrawCommand, FilterExecutionReport, FilterGraph, HeadlessRendererProvider,
    MediaError, RenderTargetFormat, Renderer2DProvider, RendererCreateRequest,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{LayerKind, LayerState, StageModel, TextWindowState, VnError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VnPresentationExecutionRequest {
    pub stage: StageModel,
    pub filters: Option<FilterGraph>,
    pub profile: String,
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
        let renderer_provider = HeadlessRendererProvider;
        let renderer_descriptor = renderer_provider.descriptor();
        let mut renderer = renderer_provider
            .create(RendererCreateRequest {
                width: request.stage.viewport_width,
                height: request.stage.viewport_height,
                format: RenderTargetFormat::Rgba8Srgb,
                profile: request.profile,
            })
            .map_err(media_error_to_vn_error)?;

        let draw_commands = stage_draw_commands(&request.stage);
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
    Ok(())
}

fn stage_draw_commands(stage: &StageModel) -> Vec<DrawCommand> {
    let mut commands = vec![DrawCommand::clear([0, 0, 0, 255])];
    let mut layers = stage
        .layers
        .iter()
        .filter(|layer| layer.visible)
        .collect::<Vec<_>>();
    layers.sort_by(|left, right| left.z.cmp(&right.z).then(left.id.cmp(&right.id)));
    for layer in layers {
        commands.push(layer_draw_command(stage, layer));
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
    commands
}

fn layer_draw_command(stage: &StageModel, layer: &LayerState) -> DrawCommand {
    let (x, y, width, height) = layer_rect(stage, layer);
    DrawCommand::rect(layer.id.clone(), x, y, width, height, layer_color(layer))
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

fn layer_color(layer: &LayerState) -> [u8; 4] {
    let mut payload = Vec::new();
    payload.extend_from_slice(layer.id.as_bytes());
    payload.extend_from_slice(format!("{:?}", layer.kind).as_bytes());
    if let Some(asset) = &layer.asset {
        payload.extend_from_slice(asset.as_bytes());
    }
    let hash = Hash256::from_sha256(&payload);
    let bytes = hash.as_bytes();
    let alpha = (255.0 * layer.opacity.clamp(0.0, 1.0)) as u8;
    [bytes[0].max(16), bytes[1].max(16), bytes[2].max(16), alpha]
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
