//! GPU filter product module owned by AstraEMU Manager.
//!
//! The host owns the Slint/wgpu device, queue, and RGBA8 textures. This module
//! owns only effect metadata, shader compilation, transient intermediates, and
//! dispatch. A failed reload never replaces the currently active chain.

mod compiler;
mod engine;
pub mod format4;
mod generator;

pub use engine::{
    FilterConfiguration, FilterEngine, FilterError, FilterPreset, DXC_VERSION, MAGPIE_REVISION,
};
