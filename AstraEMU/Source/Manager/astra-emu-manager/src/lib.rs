mod gamepad;
mod host;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
pub mod desktop_source;
pub mod family_host;

extern crate self as astra_emu_manager;

pub use host::{
    run_manager, run_manager_with_initial_state, AstraUnderlayRenderer, HostError,
    ManagerController, TranslationOverlayView, WgpuFrameContext,
};

#[cfg(target_os = "android")]
include!("main.rs");
