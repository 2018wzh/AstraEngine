use std::collections::BTreeSet;
use std::time::Instant;

use astra_core::{Diagnostic, Hash256};
use astra_ui_core::{
    UiActionEnvelope, UiBackend, UiBackendDescriptor, UiButtonState, UiCapability, UiFrameOutput,
    UiFrameRequest, UiInputDisposition, UiInputDispositionKind, UiInputEvent, UiInputEventKind,
    UiNavigationAction, UiPerformanceSample, UiSemanticAction, UiSemanticNode, UiSemanticSnapshot,
    UiValidationError, ValidateUi,
};
use yakui_core::geometry::{Rect, Vec2};
use yakui_core::{WidgetId, Yakui};

use crate::{AstraYakuiInputRouter, YakuiPaintConverter};

pub struct YakuiViewOutput {
    pub actions: Vec<UiActionEnvelope>,
    pub repaint_after_ns: Option<u64>,
    pub diagnostics: Vec<Diagnostic>,
    pub instantiated_nodes: u32,
    pub active_texture_bytes: u64,
    pub force_consumed_sequences: BTreeSet<u64>,
    pub focus_widget: Option<WidgetId>,
}

pub trait YakuiViewRenderer: Send {
    fn request_focus(&mut self, semantic_id: String);

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
    last_semantics: Option<UiSemanticSnapshot>,
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
        tracing::info!(
            event = "ui.backend.create",
            provider_id = %descriptor.provider_id,
            provider_version = %descriptor.provider_version,
            artifact_fingerprint = %descriptor.artifact_fingerprint,
            capability_count = descriptor.capabilities.len(),
            "created Yakui UI backend"
        );
        Ok(Self {
            descriptor,
            yakui: Yakui::new(),
            input: AstraYakuiInputRouter::default(),
            paint: YakuiPaintConverter::new(),
            renderer,
            live_session: None,
            live_generation: 0,
            last_semantics: None,
            shutdown: false,
        })
    }

    pub fn renderer_mut(&mut self) -> &mut R {
        &mut self.renderer
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
        let request_validation_started = Instant::now();
        request.validate()?;
        let request_validation_ns = request_validation_started
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        let mut request = request;
        if let Some(semantics) = self.last_semantics.as_ref() {
            if let Some(focused) = semantics.nodes.iter().find(|node| node.focused) {
                for event in &mut request.input.events {
                    let action = match &event.kind {
                        UiInputEventKind::Keyboard {
                            physical_key,
                            state: UiButtonState::Pressed,
                            repeat: false,
                            ..
                        } if focused.role == astra_ui_core::UiSemanticRole::Slider => {
                            match physical_key.as_str() {
                                "ArrowLeft" | "ArrowDown" => Some("decrement"),
                                "ArrowRight" | "ArrowUp" => Some("increment"),
                                _ => None,
                            }
                        }
                        UiInputEventKind::Keyboard {
                            physical_key,
                            state: UiButtonState::Pressed,
                            repeat: false,
                            ..
                        } if focused.role == astra_ui_core::UiSemanticRole::Toggle
                            && matches!(physical_key.as_str(), "Enter" | "Space") =>
                        {
                            Some("activate")
                        }
                        UiInputEventKind::Navigation {
                            action: UiNavigationAction::Activate,
                        } if focused.role == astra_ui_core::UiSemanticRole::Toggle => {
                            Some("activate")
                        }
                        _ => None,
                    };
                    if let Some(action) = action {
                        event.kind = UiInputEventKind::AccessibilityAction {
                            semantic_id: focused.id.clone(),
                            action: action.to_string(),
                            value: None,
                        };
                    }
                }
            }
        }
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
        if self.live_session.is_none() {
            tracing::info!(
                event = "ui.backend.session.start",
                provider_id = %self.descriptor.provider_id,
                session_id = %request.session_id,
                generation = request.generation,
                "started Yakui UI session"
            );
            self.live_session = Some(request.session_id.clone());
        }
        self.live_generation = request.generation;
        self.yakui.set_surface_size(Vec2::new(
            request.viewport.physical_width as f32,
            request.viewport.physical_height as f32,
        ));
        self.yakui.set_unscaled_viewport(Rect::from_pos_size(
            Vec2::ZERO,
            Vec2::new(
                request.viewport.physical_width as f32,
                request.viewport.physical_height as f32,
            ),
        ));
        self.yakui
            .set_scale_factor(request.viewport.scale_factor * request.viewport.font_scale);

        let update_started = Instant::now();
        let input_routing_started = Instant::now();
        let mut dispositions = Vec::with_capacity(request.input.events.len());
        for event in &request.input.events {
            if let Some(action) = navigation_action(event) {
                let semantic_target_id = if matches!(
                    event_button_state(event),
                    Some(UiButtonState::Pressed) | None
                ) {
                    let target = self
                        .last_semantics
                        .as_ref()
                        .and_then(|semantics| navigate_semantics(semantics, action))
                        .ok_or_else(|| {
                            UiValidationError::invalid(
                                "ASTRA_UI_NAVIGATION_TARGET_MISSING",
                                "navigation input requires a focused semantic target and a reachable focus candidate",
                            )
                        })?;
                    self.renderer.request_focus(target.clone());
                    Some(target)
                } else {
                    self.last_semantics.as_ref().and_then(|semantics| {
                        semantics
                            .nodes
                            .iter()
                            .find(|node| node.focused)
                            .map(|node| node.id.clone())
                    })
                };
                dispositions.push(UiInputDisposition {
                    sequence: event.sequence,
                    disposition: UiInputDispositionKind::Consumed,
                    semantic_target_id,
                });
            } else {
                dispositions.push(self.input.route(&mut self.yakui, event)?);
            }
        }
        let input_routing_ns = input_routing_started
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        self.yakui.start();
        let tree_build_started = Instant::now();
        let view = self.renderer.build(&mut self.yakui, &request);
        let tree_build_ns = tree_build_started
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        let layout_finalize_started = Instant::now();
        self.yakui.finish();
        let layout_finalize_ns = layout_finalize_started
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
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
        let semantics_started = Instant::now();
        let semantics = self.renderer.semantics(&self.yakui, &request)?;
        let semantics_ns = semantics_started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.last_semantics = Some(semantics.clone());
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
        let mut output = UiFrameOutput {
            schema: "astra.ui_frame_output.v1".to_string(),
            dispositions,
            actions,
            performance: UiPerformanceSample {
                request_validation_ns,
                input_routing_ns,
                tree_build_ns,
                layout_finalize_ns,
                semantics_ns,
                update_layout_ns,
                paint_conversion_ns,
                output_validation_ns: 0,
                texture_update_bytes,
                draw_calls: render.primitives.len() as u32,
                vertices,
                active_texture_bytes: view.active_texture_bytes,
                instantiated_nodes: view.instantiated_nodes,
            },
            render,
            semantics,
            repaint_after_ns: view.repaint_after_ns,
            diagnostics: view.diagnostics,
        };
        let output_validation_started = Instant::now();
        output.validate()?;
        output.performance.output_validation_ns = output_validation_started
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        tracing::trace!(
            event = "ui.backend.frame.complete",
            provider_id = %self.descriptor.provider_id,
            session_id = %request.session_id,
            generation = request.generation,
            input_count = request.input.events.len(),
            action_count = output.actions.len(),
            semantic_hash = %output.semantics.hash,
            draw_calls = output.performance.draw_calls,
            vertices = output.performance.vertices,
            texture_update_bytes = output.performance.texture_update_bytes,
            "completed Yakui UI frame"
        );
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
        tracing::info!(
            event = "ui.backend.context.restore",
            provider_id = %self.descriptor.provider_id,
            session_id,
            generation,
            "restored Yakui UI render context"
        );
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), UiValidationError> {
        self.ensure_live()?;
        tracing::info!(
            event = "ui.backend.session.shutdown",
            provider_id = %self.descriptor.provider_id,
            session_id = self.live_session.as_deref().unwrap_or("none"),
            generation = self.live_generation,
            "shut down Yakui UI session"
        );
        self.shutdown = true;
        self.live_session = None;
        Ok(())
    }
}

fn event_button_state(event: &UiInputEvent) -> Option<UiButtonState> {
    match &event.kind {
        UiInputEventKind::Keyboard { state, .. } => Some(*state),
        _ => None,
    }
}

fn navigation_action(event: &UiInputEvent) -> Option<UiNavigationAction> {
    match &event.kind {
        UiInputEventKind::Keyboard {
            physical_key,
            repeat: false,
            ..
        } => match physical_key.as_str() {
            "ArrowUp" => Some(UiNavigationAction::Up),
            "ArrowDown" => Some(UiNavigationAction::Down),
            "ArrowLeft" => Some(UiNavigationAction::Left),
            "ArrowRight" => Some(UiNavigationAction::Right),
            "Tab" => Some(UiNavigationAction::Next),
            _ => None,
        },
        UiInputEventKind::Navigation { action }
            if !matches!(
                action,
                UiNavigationAction::Activate
                    | UiNavigationAction::Cancel
                    | UiNavigationAction::PagePrevious
                    | UiNavigationAction::PageNext
            ) =>
        {
            Some(*action)
        }
        _ => None,
    }
}

fn navigate_semantics(
    semantics: &UiSemanticSnapshot,
    action: UiNavigationAction,
) -> Option<String> {
    let focusable = semantics
        .nodes
        .iter()
        .filter(|node| {
            node.enabled && !node.hidden && node.actions.contains(&UiSemanticAction::Focus)
        })
        .collect::<Vec<_>>();
    if focusable.is_empty() {
        return None;
    }
    let current = semantics
        .nodes
        .iter()
        .find(|node| node.focused && node.actions.contains(&UiSemanticAction::Focus));
    let Some(current_index) = current.and_then(|node| {
        focusable
            .iter()
            .position(|candidate| candidate.id == node.id)
    }) else {
        return match action {
            UiNavigationAction::Previous | UiNavigationAction::Up | UiNavigationAction::Left => {
                focusable.last().map(|node| node.id.clone())
            }
            _ => focusable.first().map(|node| node.id.clone()),
        };
    };
    let current = focusable[current_index];
    match action {
        UiNavigationAction::Next => focusable
            .get((current_index + 1) % focusable.len())
            .map(|node| node.id.clone()),
        UiNavigationAction::Previous => focusable
            .get((current_index + focusable.len() - 1) % focusable.len())
            .map(|node| node.id.clone()),
        UiNavigationAction::Up
        | UiNavigationAction::Down
        | UiNavigationAction::Left
        | UiNavigationAction::Right => directional_semantic(current, &focusable, action),
        UiNavigationAction::Activate
        | UiNavigationAction::Cancel
        | UiNavigationAction::PagePrevious
        | UiNavigationAction::PageNext => None,
    }
}

fn directional_semantic(
    current: &UiSemanticNode,
    focusable: &[&UiSemanticNode],
    action: UiNavigationAction,
) -> Option<String> {
    let current_center = (
        (current.bounds_points.min.x + current.bounds_points.max.x) * 0.5,
        (current.bounds_points.min.y + current.bounds_points.max.y) * 0.5,
    );
    let directional = focusable
        .iter()
        .copied()
        .filter(|candidate| candidate.id != current.id)
        .filter_map(|candidate| {
            let center = (
                (candidate.bounds_points.min.x + candidate.bounds_points.max.x) * 0.5,
                (candidate.bounds_points.min.y + candidate.bounds_points.max.y) * 0.5,
            );
            let delta = (center.0 - current_center.0, center.1 - current_center.1);
            let (primary, secondary) = match action {
                UiNavigationAction::Up if delta.1 < 0.0 => (-delta.1, delta.0.abs()),
                UiNavigationAction::Down if delta.1 > 0.0 => (delta.1, delta.0.abs()),
                UiNavigationAction::Left if delta.0 < 0.0 => (-delta.0, delta.1.abs()),
                UiNavigationAction::Right if delta.0 > 0.0 => (delta.0, delta.1.abs()),
                _ => return None,
            };
            Some((primary, secondary, candidate.id.as_str()))
        })
        .min_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.total_cmp(&right.1))
                .then_with(|| left.2.cmp(right.2))
        })
        .map(|(_, _, id)| id.to_string());
    directional.or_else(|| {
        let current_index = focusable.iter().position(|node| node.id == current.id)?;
        let fallback_index = match action {
            UiNavigationAction::Down | UiNavigationAction::Right => {
                (current_index + 1).min(focusable.len() - 1)
            }
            UiNavigationAction::Up | UiNavigationAction::Left => current_index.saturating_sub(1),
            _ => current_index,
        };
        Some(focusable[fallback_index].id.clone())
    })
}
