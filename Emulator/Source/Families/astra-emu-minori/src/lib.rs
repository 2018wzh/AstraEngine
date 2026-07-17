//! Minori PAZ virtual filesystem and lossless script research parser.

mod cache;
mod paz;
mod script;

pub use cache::*;
pub use paz::*;
pub use script::*;

pub const MINORI_READER_ID: &str = "astra.emu.minori.paz.v1";
pub const MINORI_SCRIPT_IR_SCHEMA: &str = "astra.emu.minori.script_ir.v1";
