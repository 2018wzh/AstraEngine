//! Minori PAZ virtual filesystem and lossless script research parser.

mod avi;
mod factory;
#[cfg(feature = "dynamic-plugin-export")]
mod ffi;
mod image_container;
mod paz;
mod provider;
mod runtime;
mod save;
mod script;
mod text_surface;

pub use avi::*;
pub use factory::*;
#[cfg(feature = "dynamic-plugin-export")]
pub use ffi::*;
pub use image_container::*;
pub use paz::*;
pub use provider::*;
pub use runtime::*;
pub use script::*;

/// Bumped whenever a decoded PAZ byte contract changes so stale plaintext
/// cache entries cannot cross reader implementations.
pub const MINORI_READER_ID: &str = "astra.emu.minori.paz.v3";
pub const MINORI_DECRYPT_PROVIDER_ID: &str = "astra.emu.minori.paz.decrypt.v2";
pub const MINORI_DECRYPT_DESCRIPTOR_SCHEMA: &str = "astra.emu.minori.paz.decrypt_descriptor.v1";
pub const MINORI_FAMILY_OPTIONS_SCHEMA: &str = "astra.emu.minori.mount_options.v1";
pub const MINORI_PRIVATE_PROFILE_SCHEMA: &str = "astra.emu.minori.private_profile.v2";
pub const MINORI_SCRIPT_IR_SCHEMA: &str = "astra.emu.minori.script_ir.v2";
