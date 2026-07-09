//! AstraEngine MCP (Model Context Protocol) subsystem.
//!
//! 提供 MCP tool descriptor、ContextPack、
//! permission audit 和 command allowlist。

pub mod context_pack;
pub mod session;
pub mod tool_descriptor;

pub use context_pack::*;
pub use session::*;
pub use tool_descriptor::*;
