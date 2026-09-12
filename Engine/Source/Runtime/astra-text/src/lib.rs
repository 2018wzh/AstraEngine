//! Font loading, shaping, layout and glyph resource ownership without an Engine session.

mod contract;
mod layout_engine;
mod provider;
mod resources;
mod shaping;
mod validation;
mod vertical;

pub use astra_media_core::MediaError;
pub use contract::*;
pub use provider::CosmicTextLayoutProvider;
pub use resources::*;
