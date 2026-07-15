use astra_core::{Diagnostic, Hash256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    UiActionEnvelope, UiInputDisposition, UiInputFrame, UiRenderFrame, UiSemanticSnapshot,
    UiThemeManifest, UiValidationError, UiViewport, ValidateUi, MAX_NODES_PER_VIEW,
    MAX_TEXTURE_BYTES,
};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum UiCapability {
    Pointer,
    Keyboard,
    Ime,
    Touch,
    GamepadNavigation,
    Accessibility,
    VirtualList,
    VirtualGrid,
    NineSlice,
    Canvas,
    TextInput,
    VerticalText,
    ComponentSlots,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UiBackendDescriptor {
    pub schema: String,
    pub provider_id: String,
    pub provider_version: String,
    pub input_protocol: String,
    pub render_protocol: String,
    pub capabilities: Vec<UiCapability>,
    pub artifact_fingerprint: Hash256,
    pub packaged_eligible: bool,
}

impl ValidateUi for UiBackendDescriptor {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_backend_descriptor.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BACKEND_SCHEMA",
                "backend descriptor schema must be astra.ui_backend_descriptor.v1",
            ));
        }
        crate::validate_id("backend.provider_id", &self.provider_id)?;
        crate::validate_string("backend.provider_version", &self.provider_version)?;
        crate::validate_id("backend.input_protocol", &self.input_protocol)?;
        crate::validate_id("backend.render_protocol", &self.render_protocol)?;
        let mut capabilities = self.capabilities.clone();
        capabilities.sort_unstable();
        capabilities.dedup();
        if capabilities.len() != self.capabilities.len() {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_BACKEND_CAPABILITY_DUPLICATE",
                "backend capabilities must be unique",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiFrameRequest {
    pub schema: String,
    pub session_id: String,
    pub generation: u64,
    pub viewport: UiViewport,
    pub fixed_time_ns: u64,
    pub input: UiInputFrame,
    pub theme: UiThemeManifest,
    pub model_schema: String,
    pub model_payload: Vec<u8>,
}

impl ValidateUi for UiFrameRequest {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_frame_request.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_FRAME_REQUEST_SCHEMA",
                "frame request schema must be astra.ui_frame_request.v1",
            ));
        }
        crate::validate_id("frame.session_id", &self.session_id)?;
        crate::validate_id("frame.model_schema", &self.model_schema)?;
        self.viewport.validate()?;
        self.input.validate()?;
        self.theme.validate()?;
        crate::validate_serialized_size(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiPerformanceSample {
    pub update_layout_ns: u64,
    pub paint_conversion_ns: u64,
    pub texture_update_bytes: u64,
    pub draw_calls: u32,
    pub vertices: u32,
    pub active_texture_bytes: u64,
    pub instantiated_nodes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UiFrameOutput {
    pub schema: String,
    pub dispositions: Vec<UiInputDisposition>,
    pub actions: Vec<UiActionEnvelope>,
    pub render: UiRenderFrame,
    pub semantics: UiSemanticSnapshot,
    pub repaint_after_ns: Option<u64>,
    pub performance: UiPerformanceSample,
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidateUi for UiFrameOutput {
    fn validate(&self) -> Result<(), UiValidationError> {
        if self.schema != "astra.ui_frame_output.v1" {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_FRAME_OUTPUT_SCHEMA",
                "frame output schema must be astra.ui_frame_output.v1",
            ));
        }
        let mut previous = None;
        for disposition in &self.dispositions {
            if previous.is_some_and(|sequence| sequence >= disposition.sequence) {
                return Err(UiValidationError::invalid(
                    "ASTRA_UI_DISPOSITION_SEQUENCE",
                    "input dispositions must be strictly ordered",
                ));
            }
            if let Some(id) = &disposition.semantic_target_id {
                crate::validate_id("disposition.semantic_target_id", id)?;
            }
            previous = Some(disposition.sequence);
        }
        for action in &self.actions {
            action.validate()?;
        }
        self.render.validate()?;
        self.semantics.validate()?;
        if self.render.session_id != self.semantics.session_id
            || self.render.generation != self.semantics.generation
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_FRAME_IDENTITY",
                "render and semantic frame identity mismatch",
            ));
        }
        let texture_update_bytes = self
            .render
            .textures
            .uploads
            .iter()
            .map(|upload| upload.pixels.len() as u64)
            .sum::<u64>();
        let vertices = self
            .render
            .primitives
            .iter()
            .map(|primitive| primitive.vertices.len() as u64)
            .sum::<u64>();
        if self.performance.draw_calls as usize != self.render.primitives.len()
            || u64::from(self.performance.vertices) != vertices
            || self.performance.texture_update_bytes != texture_update_bytes
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_PERFORMANCE_COUNTER_MISMATCH",
                "performance counters do not match the validated render frame",
            ));
        }
        if self.performance.active_texture_bytes > MAX_TEXTURE_BYTES as u64
            || self.performance.instantiated_nodes > MAX_NODES_PER_VIEW as u32
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_PERFORMANCE_RESOURCE_LIMIT",
                "performance resource counters exceed the UI hard limits",
            ));
        }
        if self.performance.update_layout_ns > 60_000_000_000
            || self.performance.paint_conversion_ns > 60_000_000_000
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_PERFORMANCE_DURATION_LIMIT",
                "performance duration counter exceeds the bounded sample range",
            ));
        }
        crate::validate_serialized_size(self)
    }
}

pub trait UiBackend {
    fn descriptor(&self) -> &UiBackendDescriptor;
    fn render_frame(&mut self, request: UiFrameRequest)
        -> Result<UiFrameOutput, UiValidationError>;
    fn context_restored(
        &mut self,
        session_id: &str,
        generation: u64,
    ) -> Result<(), UiValidationError>;
    fn shutdown(&mut self) -> Result<(), UiValidationError>;
}
