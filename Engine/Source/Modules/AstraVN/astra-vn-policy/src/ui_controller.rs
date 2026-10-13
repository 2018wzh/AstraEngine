use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

use astra_policy::{create_sandboxed_lua, PolicyExecutionBudget};

use astra_ui_core::{UiValue, MAX_EFFECTS_PER_CALL};

use astra_vn_ui::{
    VnUiAction, VnUiControllerEffect, VnUiControllerManifest, VnUiControllerSnapshot,
    VnUiControllerUpdate, VnUiSessionState,
};

#[cfg(feature = "portable-luau-runtime")]
use luaur_rt as mlua;

#[cfg(feature = "portable-luau-runtime")]
use luaur_rt::{Lua, LuaSerdeExt, Table, Value};

#[cfg(feature = "luau-runtime")]
use mlua::{Compiler, Lua, LuaSerdeExt, Table, Value};

use thiserror::Error;

#[cfg(test)]
mod tests;

mod contract;
use contract::ControllerInvocation;
pub use contract::LuauUiControllerError;
pub use contract::LuauUiControllerHost;
mod lua_value_to_ui_value;
#[cfg(feature = "luau-runtime")]
use lua_value_to_ui_value::create_table;
#[cfg(feature = "portable-luau-runtime")]
use lua_value_to_ui_value::create_table;
use lua_value_to_ui_value::lua_value_to_ui_value;
use lua_value_to_ui_value::runtime_error;
use lua_value_to_ui_value::validate_effects;
use lua_value_to_ui_value::validate_handler;
