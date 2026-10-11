//! Window-thread cursor ownership. The Slint stage exports its actual viewport;
//! native warping and input therefore use the same letterbox and DPI geometry.
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use astra_emu_family_api::FamilyPointerRequest;
use astra_emu_manager_ui_slint::{ManagerWindow, SlintManagerAdapter};
use slint::{winit_030::WinitWindowAccessor, ComponentHandle};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::{dpi::PhysicalPosition, event::WindowEvent};

pub(super) struct HostPointer {
    visible: bool,
    autohide: Option<Duration>,
    idle_hidden: bool,
    last_activity: Instant,
    position: Option<PhysicalPosition<f64>>,
    inside_window: bool,
    was_active: bool,
    displayed: bool,
    timer: slint::Timer,
}

impl HostPointer {
    pub(super) fn install(adapter: &Rc<SlintManagerAdapter>) -> Rc<RefCell<Self>> {
        let pointer = Rc::new(RefCell::new(Self {
            visible: true,
            autohide: None,
            idle_hidden: false,
            last_activity: Instant::now(),
            position: None,
            inside_window: false,
            was_active: false,
            displayed: true,
            timer: slint::Timer::default(),
        }));
        let weak_pointer = Rc::downgrade(&pointer);
        let weak_window = adapter.window().as_weak();
        adapter.window().on_pointer_state_changed(move || {
            if let (Some(pointer), Some(window)) = (weak_pointer.upgrade(), weak_window.upgrade()) {
                Self::synchronize(&pointer, &window);
            }
        });
        pointer
    }

    pub(super) fn apply(
        pointer: &Rc<RefCell<Self>>,
        window: &ManagerWindow,
        request: FamilyPointerRequest,
    ) -> Result<(), String> {
        window
            .window()
            .with_winit_window(|native| {
                let handle = native
                    .window_handle()
                    .map_err(|_| "ASTRA_EMU_HOST_WINDOW_UNAVAILABLE".to_owned())?;
                if !matches!(
                    handle.as_raw(),
                    RawWindowHandle::Win32(_)
                        | RawWindowHandle::Xlib(_)
                        | RawWindowHandle::Xcb(_)
                        | RawWindowHandle::Wayland(_)
                        | RawWindowHandle::AppKit(_)
                ) {
                    return Err("ASTRA_EMU_HOST_POINTER_UNAVAILABLE".to_owned());
                }
                Ok(())
            })
            .ok_or_else(|| "ASTRA_EMU_HOST_WINDOW_UNAVAILABLE".to_owned())??;
        let autohide = request.autohide_ms.into_option().map(Duration::from_millis);
        if autohide.is_some_and(|duration| Instant::now().checked_add(duration).is_none()) {
            return Err("ASTRA_EMU_HOST_POINTER_DURATION".into());
        }
        let x = request.logical_x.into_option();
        let y = request.logical_y.into_option();
        if x.is_some() || y.is_some() {
            let current = pointer.borrow().position;
            let rect = [
                window.get_game_content_window_x() as f64,
                window.get_game_content_window_y() as f64,
                window.get_game_content_window_width() as f64,
                window.get_game_content_window_height() as f64,
            ];
            let stage = [
                window.get_stage_native_width() as f64,
                window.get_stage_native_height() as f64,
            ];
            let position = window
                .window()
                .with_winit_window(|native| {
                    let position = map_position(x, y, current, rect, stage, native.scale_factor())?;
                    native
                        .set_cursor_position(position)
                        .map_err(|_| "ASTRA_EMU_HOST_POINTER_POSITION_UNAVAILABLE".to_owned())?;
                    Ok::<_, String>(position)
                })
                .ok_or_else(|| "ASTRA_EMU_HOST_WINDOW_UNAVAILABLE".to_owned())??;
            pointer.borrow_mut().position = Some(position);
        }
        let visible = request.visible.into_option();
        {
            let mut state = pointer.borrow_mut();
            if let Some(visible) = visible {
                state.visible = visible;
            }
            if let Some(duration) = autohide {
                state.autohide = (!duration.is_zero()).then_some(duration);
            }
            if visible.is_some() || autohide.is_some() || x.is_some() || y.is_some() {
                state.idle_hidden = false;
                state.last_activity = Instant::now();
            }
        }
        Self::synchronize(pointer, window);
        Ok(())
    }

    pub(super) fn observe(
        pointer: &Rc<RefCell<Self>>,
        window: &ManagerWindow,
        event: &WindowEvent,
    ) {
        {
            let mut state = pointer.borrow_mut();
            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    state.position = Some(*position);
                    state.inside_window = true;
                    state.last_activity = Instant::now();
                    state.idle_hidden = false;
                }
                WindowEvent::MouseInput { .. } => {
                    state.last_activity = Instant::now();
                    state.idle_hidden = false;
                }
                WindowEvent::CursorEntered { .. } => state.inside_window = true,
                WindowEvent::CursorLeft { .. } => state.inside_window = false,
                WindowEvent::Focused(_) => {}
                _ => return,
            }
        }
        Self::synchronize(pointer, window);
    }

    fn synchronize(pointer: &Rc<RefCell<Self>>, window: &ManagerWindow) {
        let mut state = pointer.borrow_mut();
        state.timer.stop();
        let active = window.get_game_active();
        if !active && state.was_active {
            state.visible = true;
            state.autohide = None;
            state.idle_hidden = false;
        }
        state.was_active = active;
        let game_visible = state.visible && !state.idle_hidden;
        window.set_game_cursor_visible(game_visible);
        let controlling = active
            && !window.get_translation_overlay_active()
            && !window.get_diagnostics_overlay_active()
            && !window.get_filters_overlay_active()
            && state.inside_window
            && window
                .window()
                .with_winit_window(|native| {
                    native.has_focus()
                        && state.position.is_some_and(|position| {
                            let x = position.x / native.scale_factor();
                            let y = position.y / native.scale_factor();
                            let left = window.get_game_content_window_x() as f64;
                            let top = window.get_game_content_window_y() as f64;
                            x >= left
                                && y >= top
                                && x < left + window.get_game_content_window_width() as f64
                                && y < top + window.get_game_content_window_height() as f64
                        })
                })
                .unwrap_or(false);
        let displayed = !controlling || game_visible;
        if displayed != state.displayed {
            window
                .window()
                .with_winit_window(|native| native.set_cursor_visible(displayed));
            state.displayed = displayed;
        }
        if controlling && game_visible {
            if let Some(duration) = state.autohide {
                let delay = duration.saturating_sub(state.last_activity.elapsed());
                let weak_pointer = Rc::downgrade(pointer);
                let weak_window = window.as_weak();
                state
                    .timer
                    .start(slint::TimerMode::SingleShot, delay, move || {
                        if let (Some(pointer), Some(window)) =
                            (weak_pointer.upgrade(), weak_window.upgrade())
                        {
                            pointer.borrow_mut().idle_hidden = true;
                            Self::synchronize(&pointer, &window);
                        }
                    });
            }
        }
    }
}

fn map_position(
    x: Option<i32>,
    y: Option<i32>,
    current: Option<PhysicalPosition<f64>>,
    rect: [f64; 4],
    stage: [f64; 2],
    scale: f64,
) -> Result<PhysicalPosition<f64>, String> {
    if !rect
        .into_iter()
        .chain(stage)
        .chain([scale])
        .all(f64::is_finite)
        || rect[2] <= 0.0
        || rect[3] <= 0.0
        || stage[0] <= 0.0
        || stage[1] <= 0.0
        || scale <= 0.0
    {
        return Err("ASTRA_EMU_HOST_POINTER_VIEWPORT_UNAVAILABLE".into());
    }
    let missing = || "ASTRA_EMU_HOST_POINTER_POSITION_UNAVAILABLE".to_owned();
    let x = match x {
        Some(x) => (rect[0] + x as f64 / stage[0] * rect[2]) * scale,
        None => current.ok_or_else(missing)?.x,
    };
    let y = match y {
        Some(y) => (rect[1] + y as f64 / stage[1] * rect[3]) * scale,
        None => current.ok_or_else(missing)?.y,
    };
    Ok(PhysicalPosition::new(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage_warp_uses_content_viewport_dpi_and_preserves_other_axis() {
        let rect = [100.0, 80.0, 640.0, 360.0];
        assert_eq!(
            map_position(Some(640), Some(360), None, rect, [1280.0, 720.0], 1.5).unwrap(),
            PhysicalPosition::new(630.0, 390.0)
        );
        assert_eq!(
            map_position(
                Some(0),
                None,
                Some(PhysicalPosition::new(50.0, 123.0)),
                rect,
                [1280.0, 720.0],
                1.5
            )
            .unwrap(),
            PhysicalPosition::new(150.0, 123.0)
        );
        assert!(map_position(None, Some(0), None, rect, [1280.0, 720.0], 1.0).is_err());
        assert!(map_position(Some(0), Some(0), None, rect, [0.0, 720.0], 1.0).is_err());
    }
}
