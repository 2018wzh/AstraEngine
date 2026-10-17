use astra_emu_manager_core::model::FamilyWindowIcon;
use slint::winit_030::WinitWindowAccessor;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Let the window manager fit the initial window when the preferred UI size
/// exceeds the current display, including its native decorations.
pub(super) fn fit_initial_window(window: &slint::Window) {
    let oversized = window.with_winit_window(|window| {
        window.current_monitor().is_some_and(|monitor| {
            let available = monitor.size();
            let requested = window.outer_size();
            requested.width > available.width || requested.height > available.height
        })
    });
    if oversized == Some(true) {
        window.set_maximized(true);
    }
}

/// The Manager has no custom application icon; None restores the window class default.
pub(super) fn set_icon(
    window: &slint::Window,
    icon: Option<FamilyWindowIcon>,
) -> Result<(), String> {
    let icon = icon
        .map(|icon| {
            icon.validate().map_err(|error| error.to_string())?;
            winit::window::Icon::from_rgba(icon.rgba, icon.width, icon.height)
                .map_err(|_| "ASTRA_EMU_HOST_WINDOW_ICON_INVALID".to_owned())
        })
        .transpose()?;
    window
        .with_winit_window(|window| {
            if icon.is_some() {
                let handle = window
                    .window_handle()
                    .map_err(|_| "ASTRA_EMU_HOST_WINDOW_UNAVAILABLE")?;
                if !matches!(
                    handle.as_raw(),
                    RawWindowHandle::Win32(_) | RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)
                ) {
                    return Err("ASTRA_EMU_HOST_WINDOW_ICON_UNAVAILABLE".to_owned());
                }
            }
            window.set_window_icon(icon);
            Ok(())
        })
        .ok_or_else(|| "ASTRA_EMU_HOST_WINDOW_UNAVAILABLE".to_owned())?
}

pub(super) fn execute(
    window: &astra_emu_manager_ui_slint::ManagerWindow,
    pointer: &std::rc::Rc<std::cell::RefCell<super::pointer::HostPointer>>,
    command: &crate::window_service::WindowCommand,
) -> Result<crate::window_service::WindowOutcome, String> {
    use astra_emu_abi as abi;
    use crate::window_service::{WindowCommand, WindowOutcome};
    use slint::ComponentHandle;
    match command {
        WindowCommand::Icon(icon) => set_icon(window.window(), Some(icon.clone()))?,
        WindowCommand::Pointer(request) => super::pointer::HostPointer::apply(pointer, window, request.clone())?,
        _ => {}
    }
    window.window().with_winit_window(|native| {
        match command {
            WindowCommand::Title(title) => { native.set_title(title); window.set_game_title(title.as_str().into()); window.set_has_game_title(true); }
            WindowCommand::Fullscreen { mode, width, height } => {
                let fullscreen = match *mode {
                    abi::FULLSCREEN_WINDOWED => None,
                    abi::FULLSCREEN_BORDERLESS => Some(winit::window::Fullscreen::Borderless(native.current_monitor())),
                    abi::FULLSCREEN_EXCLUSIVE => {
                        let monitor = native.current_monitor().ok_or("ASTRA_EMU_WINDOW_MONITOR")?;
                        let mode = monitor.video_modes().filter(|mode| mode.size() == winit::dpi::PhysicalSize::new(*width, *height)).max_by_key(|mode| (mode.refresh_rate_millihertz(), mode.bit_depth())).ok_or("ASTRA_EMU_WINDOW_DISPLAY_MODE")?;
                        Some(winit::window::Fullscreen::Exclusive(mode))
                    }
                    _ => return Err("ASTRA_EMU_WINDOW_FULLSCREEN".into()),
                };
                native.set_fullscreen(fullscreen);
            }
            WindowCommand::Size { width, height } => { let _ = native.request_inner_size(winit::dpi::PhysicalSize::new(*width, *height)); }
            WindowCommand::Position { x, y } => native.set_outer_position(winit::dpi::PhysicalPosition::new(*x, *y)),
            WindowCommand::Visibility(value) => native.set_visible(*value),
            WindowCommand::Minimized(value) => native.set_minimized(*value),
            WindowCommand::Icon(_) | WindowCommand::Pointer(_) => {}
        }
        let size = native.inner_size();
        let mut result = WindowOutcome { valid_fields: abi::WINDOW_RESULT_SIZE | abi::WINDOW_RESULT_MODE, width: size.width, height: size.height,
            fullscreen: match native.fullscreen() { None => abi::FULLSCREEN_WINDOWED, Some(winit::window::Fullscreen::Borderless(_)) => abi::FULLSCREEN_BORDERLESS, Some(winit::window::Fullscreen::Exclusive(_)) => abi::FULLSCREEN_EXCLUSIVE }, ..WindowOutcome::default() };
        if let Ok(position) = native.outer_position() { result.valid_fields |= abi::WINDOW_RESULT_POSITION; result.x = position.x; result.y = position.y; }
        if let Some(visible) = native.is_visible() { result.valid_fields |= abi::WINDOW_RESULT_VISIBILITY; result.visible = u32::from(visible); }
        if let Some(minimized) = native.is_minimized() { result.valid_fields |= abi::WINDOW_RESULT_MINIMIZED; result.minimized = u32::from(minimized); }
        Ok(result)
    }).ok_or_else(|| "ASTRA_EMU_HOST_WINDOW_UNAVAILABLE".to_owned())?
}
