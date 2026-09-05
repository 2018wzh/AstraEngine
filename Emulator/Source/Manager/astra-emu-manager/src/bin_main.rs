// The desktop binary root. `main.rs` is shared with the Android cdylib via
// `include!` from lib.rs, and crate-level attributes are only valid in this
// bin root, so they live here.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

include!("main.rs");
