//! Owned Rust data for Manager UI, persistence and presentation. Never an ABI.
mod config;
mod descriptor;
mod frame;
mod input;
mod key_names;
mod modal;
mod window;
pub use config::*;
pub use descriptor::*;
pub use frame::*;
pub use input::*;
pub use modal::*;
pub use window::*;

pub fn validate_dimensions(width: u32, height: u32) -> FamilyResult<()> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(FamilyError::invalid(
            "ASTRA_EMU_FRAME_SIZE",
            "invalid dimensions",
        ));
    }
    Ok(())
}
