use std::collections::BTreeSet;
use std::time::Instant;

use astra_core::{Diagnostic, Hash256};
use astra_ui_core::{
    UiActionEnvelope, UiBackend, UiBackendDescriptor, UiCapability, UiFrameOutput, UiFrameRequest,
    UiPerformanceSample, UiSemanticSnapshot, UiValidationError, ValidateUi,
};
use yakui_core::geometry::Vec2;
use yakui_core::{WidgetId, Yakui};

use crate::{AstraYakuiInputRouter, YakuiPaintConverter};

pub struct YakuiViewOutput {
    pub actions: Vec<UiActionEnvelope>,
    pub repaint_after_ns: Option<u64>,
    pub diagnostics: Vec<Diagnostic>,
    pub instantiated_nodes: u32,
    pub force_consumed_sequences: BTreeSet<u64>,
    pub focus_widget: Option<WidgetId>,
}

pub trait YakuiViewRenderer: Send {
    fn build(
        &mut self,
        yakui: &mut Yakui,
        request: &UiFrameRequest,
    ) -> Result<YakuiViewOutput, UiValidationError>;

    fn semantics(
        &mut self,
        yakui: &Yakui,
        request: &UiFrameRequest,
    ) -> Result<UiSemanticSnapshot, UiValidationError>;
}

pub struct AstraYakuiBackend<R> {
    descriptor: UiBackendDescriptor,
    yakui: Yakui,
    input: AstraYakuiInputRouter,
    paint: YakuiPaintConverter,
    renderer: R,
    live_session: Option<String>,
    live_generation: u64,
    shutdown: bool,
}

impl<R: YakuiViewRenderer> AstraYakuiBackend<R> {
    pub fn new(renderer: R, artifact_fingerprint: Hash256) -> Result<Self, UiValidationError> {
        let descriptor = UiBackendDescriptor {
            schema: "astra.ui_backend_descriptor.v1".to_string(),
            provider_id: "astra.ui.yakui".to_string(),
            provider_version: env!("CARGO_PKG_VERSION").to_string(),
            input_protocol: "astra.ui_input_frame.v1".to_string(),
            render_protocol: "astra.ui_render_frame.v1".to_string(),
            capabilities: vec![
                UiCapability::Pointer,
                UiCapability::Keyboard,
                UiCapability::Ime,
                UiCapability::Touch,
                UiCapability::GamepadNavigation,
                UiCapability::Accessibility,
                UiCapability::VirtualList,
                UiCapability::VirtualGrid,
                UiCapability::NineSlice,
                UiCapability::Canvas,
                UiCapability::TextInput,
                UiCapability::VerticalText,
                UiCapability::ComponentSlots,
            ],
            artifact_fingerprint,
            packaged_eligible: true,
        };
        descriptor.validate()?;
        Ok(Self {
            descriptor,
            yakui: Yakui::new(),
            input: AstraYakuiInputRouter::default(),
            paint: YakuiPaintConverter::new(),
            renderer,
            live_session: None,
            live_generation: 0,
            shutdown: false,
        })
    }

    fn ensure_live(&self) -> Result<(), UiValidationError> {
        if self.shutdown {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_YAKUI_SHUTDOWN",
                "Yakui backend was already shut down",
            ));
        }
        Ok(())
    }
}

impl<R: YakuiViewRenderer> UiBackend for AstraYakuiBackend<R> {
    fn descriptor(&self) -> &UiBackendDescriptor {
        &self.descriptor
    }

    fn render_frame(
        &mut self,
        request: UiFrameRequest,
    ) -> Result<UiFrameOutput, UiValidationError> {
        self.ensure_live()?;
        request.validate()?;
        if self
            .live_session
            .as_deref()
            .is_some_and(|id| id != request.session_id)
        {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_YAKUI_SESSION_CONFLICT",
                "a backend instance cannot serve multiple UI sessions",
            ));
        }
        if self.live_generation != 0 && request.generation < self.live_generation {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_YAKUI_STALE_GENERATION",
                "UI generation regressed",
            ));
        }
        self.live_session
            .get_or_insert_with(|| request.session_id.clone());
        self.live_generation = request.generation;
        self.yakui.set_surface_size(Vec2::new(
            request.viewport.physical_width as f32,
            request.viewport.physical_height as f32,
        ));
        self.yakui
            .set_scale_factor(request.viewport.scale_factor * request.viewport.font_scale);

        let update_started = Instant::now();
        let mut dispositions = Vec::with_capacity(request.input.events.len());
        for event in &request.input.events {
            dispositions.push(self.input.route(&mut self.yakui, event)?);
        }
        self.yakui.start();
        let view = self.renderer.build(&mut self.yakui, &request);
        self.yakui.finish();
        let view = view?;
        if let Some(widget_id) = view.focus_widget {
            self.yakui.request_focus(Some(widget_id));
        }
        for disposition in &mut dispositions {
            if view
                .force_consumed_sequences
                .contains(&disposition.sequence)
            {
                disposition.disposition = astra_ui_core::UiInputDispositionKind::Consumed;
            }
        }
        let semantics = self.renderer.semantics(&self.yakui, &request)?;
        let update_layout_ns = update_started.elapsed().as_nanos().min(u64::MAX as u128) as u64;

        let paint_started = Instant::now();
        let paint_dom = self.yakui.paint();
        let render = self.paint.convert(
            paint_dom,
            &request.session_id,
            request.generation,
            request.viewport.clone(),
        )?;
        let paint_conversion_ns = paint_started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let texture_update_bytes = render
            .textures
            .uploads
            .iter()
            .map(|upload| upload.pixels.len() as u64)
            .sum();
        let vertices = render
            .primitives
            .iter()
            .map(|mesh| mesh.vertices.len() as u32)
            .sum();
        let mut actions = view.actions;
        for action in &mut actions {
            action.semantic_snapshot_hash = semantics.hash;
        }
        let output = UiFrameOutput {
            schema: "astra.ui_frame_output.v1".to_string(),
            dispositions,
            actions,
            performance: UiPerformanceSample {
                update_layout_ns,
                paint_conversion_ns,
                texture_update_bytes,
                draw_calls: render.primitives.len() as u32,
                vertices,
                active_texture_bytes: 0,
                instantiated_nodes: view.instantiated_nodes,
            },
            render,
            semantics,
            repaint_after_ns: view.repaint_after_ns,
            diagnostics: view.diagnostics,
        };
        output.validate()?;
        Ok(output)
    }

    fn context_restored(
        &mut self,
        session_id: &str,
        generation: u64,
    ) -> Result<(), UiValidationError> {
        self.ensure_live()?;
        if self.live_session.as_deref() != Some(session_id) || generation < self.live_generation {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_YAKUI_RESTORE_IDENTITY",
                "context restore does not match the live session generation",
            ));
        }
        self.live_generation = generation;
        self.paint.request_full_resync();
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), UiValidationError> {
        self.ensure_live()?;
        self.shutdown = true;
        self.live_session = None;
        Ok(())
    }
}
