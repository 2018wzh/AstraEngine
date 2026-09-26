use astra_emu_family_api::FamilyWindowIcon;
use slint::winit_030::WinitWindowAccessor;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// The Manager has no custom application icon; None restores the window class default.
pub(super) fn set_icon(
    window: &slint::Window,
    icon: Option<FamilyWindowIcon>,
) -> Result<(), String> {
    let icon = icon
        .map(|icon| {
            icon.validate().map_err(|error| error.to_string())?;
            winit::window::Icon::from_rgba(icon.rgba.into_vec(), icon.width, icon.height)
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
