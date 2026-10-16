//! AstraVN authoring documents shared by desktop, ACP and MCP clients.

mod background_compile;
pub use background_compile::{BackgroundCompiler, CompilationResult};
mod document;
mod editor_metadata;
mod history;
pub mod timeline;
mod transaction;

pub use astra_vn_script::*;
pub use document::*;
pub use editor_metadata::*;
pub use transaction::*;
