//! Backend-neutral, bounded runtime UI contracts.

mod action;
mod backend;
mod blueprint;
mod input;
mod performance;
mod render;
mod semantic;
mod theme;
mod validation;

pub use action::*;
pub use backend::*;
pub use blueprint::*;
pub use input::*;
pub use performance::*;
pub use render::*;
pub use semantic::*;
pub use theme::*;
pub use validation::*;

pub const UI_CONTRACT_SCHEMA: &str = "astra.ui_contract.v1";
