pub mod effects;
mod gamepad;
mod host;
pub mod modal_service;
pub mod startup;
pub mod window_service;

extern crate self as astra_emu_manager;

pub use host::{
    run_manager, run_manager_with_initial_state, AstraUnderlayRenderer, FilterApply, HostError,
    HostExit, HostWake, ManagerController, SnackbarOutcome, StageTextureUpdate, WgpuFrameContext,
};
