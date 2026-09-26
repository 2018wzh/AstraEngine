use crate::{PolicyIoCapabilities, PolicyIoFailure, PolicyIoState, PolicyIoStatus};
use mlua::{Lua, Table};
use std::{cell::RefCell, rc::Rc};

pub(super) fn install(
    lua: &Lua,
    astra: &Table,
    state: Rc<RefCell<PolicyIoState>>,
    capabilities: PolicyIoCapabilities,
) -> mlua::Result<()> {
    let api = lua.create_table()?;
    let reads = Rc::clone(&state);
    api.set(
        "read_text",
        lua.create_function(move |_, (key, resource): (String, String)| {
            let resource = astra_asset::VfsUri::parse(&resource).map_err(mlua::Error::external)?;
            let status = reads
                .borrow_mut()
                .read_text(key, resource, &capabilities)
                .map_err(mlua::Error::external)?;
            Ok(match status {
                PolicyIoStatus::Pending => (None, "pending"),
                PolicyIoStatus::Ready(text) => (Some(text), "ready"),
                PolicyIoStatus::Failed(error) => (
                    None,
                    match error {
                        PolicyIoFailure::Host => "host_error",
                        PolicyIoFailure::Limit => "limit",
                        PolicyIoFailure::Cancelled => "cancelled",
                        PolicyIoFailure::Interrupted => "interrupted",
                    },
                ),
            })
        })?,
    )?;
    let cancellations = Rc::clone(&state);
    api.set(
        "cancel",
        lua.create_function(move |_, key: String| {
            if let Some(record) = cancellations.borrow_mut().requests.get_mut(&key) {
                if record.status == PolicyIoStatus::Pending {
                    record.status = PolicyIoStatus::Failed(PolicyIoFailure::Cancelled);
                }
            }
            Ok(())
        })?,
    )?;
    api.set(
        "release",
        lua.create_function(move |_, key: String| {
            let mut state = state.borrow_mut();
            if state
                .requests
                .get(&key)
                .is_some_and(|record| record.status == PolicyIoStatus::Pending)
            {
                return Err(mlua::Error::runtime(
                    "ASTRA_POLICY_HOST_IO_PENDING: cancel before releasing pending IO",
                ));
            }
            state.requests.remove(&key);
            Ok(())
        })?,
    )?;
    astra.set("host", api)
}
