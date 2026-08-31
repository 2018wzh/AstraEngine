//! Minori PAZ virtual filesystem and lossless script research parser.

mod avi;
mod factory;
#[cfg(feature = "dynamic-plugin-export")]
mod ffi;
mod image_container;
mod locale;
mod message;
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
pub use locale::*;
pub use message::*;
pub use paz::*;
pub use provider::*;
pub use runtime::*;
pub use script::*;

/// Bumped whenever the decoded PAZ byte contract changes.  The identity is
/// part of the manifest reader hash and does not identify a separate crypto
/// provider or cache namespace.
pub const MINORI_READER_ID: &str = "astra.emu.minori.paz.v5";
/// Retained as the family format identity required by the legacy factory ABI;
/// it is not a registry, callback or manifest provider identity.
pub const MINORI_FAMILY_OPTIONS_SCHEMA: &str = "astra.emu.minori.mount_options.v3";
pub const MINORI_SCRIPT_IR_SCHEMA: &str = "astra.emu.minori.script_ir.v2";
