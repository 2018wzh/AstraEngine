//! Minori PAZ virtual filesystem and lossless script research parser.

mod factory;
mod paz;
mod script;

pub use factory::*;
pub use paz::*;
pub use script::*;

pub const MINORI_READER_ID: &str = "astra.emu.minori.paz.v1";
pub const MINORI_DECRYPT_PROVIDER_ID: &str = "astra.emu.minori.paz.decrypt.v1";
pub const MINORI_DECRYPT_DESCRIPTOR_SCHEMA: &str = "astra.emu.minori.paz.decrypt_descriptor.v1";
pub const MINORI_FAMILY_OPTIONS_SCHEMA: &str = "astra.emu.minori.mount_options.v1";
pub const MINORI_PRIVATE_PROFILE_SCHEMA: &str = "astra.emu.minori.private_profile.v2";
pub const MINORI_SCRIPT_IR_SCHEMA: &str = "astra.emu.minori.script_ir.v1";
