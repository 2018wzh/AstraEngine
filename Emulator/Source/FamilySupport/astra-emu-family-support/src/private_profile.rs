use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use astra_core::Hash256;
use astra_emu_family_core::LegacyCoreError;
use mlua::{Lua, LuaOptions, StdLib, Table, Value, VmState};

pub const PRIVATE_PROFILE_CAPABILITY: &str = "astra.family.private_profile.v2";
const MAX_PATCH_BYTES: usize = 256 * 1024;
const MAX_PROFILE_BYTES: usize = 1024 * 1024;
const MEMORY_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const INSTRUCTION_LIMIT: u64 = 1_000_000;
const WALL_CLOCK_LIMIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct TrustedPrivateProfile {
    pub profile_id: String,
    pub schema_id: String,
    pub schema_hash: Hash256,
    pub patch_hash: Hash256,
    pub payload_hash: Hash256,
    payload: Arc<[u8]>,
}

impl TrustedPrivateProfile {
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

#[derive(Debug)]
struct Registration {
    profile_id: String,
    schema_id: String,
    payload: Vec<u8>,
}

pub fn load_private_profile(
    patch: &Path,
    expected_profile_id: &str,
    expected_schema_id: &str,
) -> Result<TrustedPrivateProfile, LegacyCoreError> {
    let source = fs::read(patch).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_PATCH_IO",
            "trusted private patch could not be read",
        )
    })?;
    if source.is_empty() || source.len() > MAX_PATCH_BYTES {
        return Err(invalid(
            "ASTRA_EMU_VFS_PATCH_SIZE",
            "trusted private patch exceeds its byte budget",
        ));
    }
    let source_text = std::str::from_utf8(&source).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_PATCH_ENCODING",
            "trusted private patch must be UTF-8",
        )
    })?;
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::BUFFER,
        LuaOptions::new(),
    )
    .map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_PATCH_VM",
            "trusted private profile VM could not be created",
        )
    })?;
    lua.set_memory_limit(MEMORY_LIMIT_BYTES).map_err(|_| {
        invalid(
            "ASTRA_EMU_VFS_PATCH_MEMORY",
            "trusted private profile memory limit could not be installed",
        )
    })?;
    let globals = lua.globals();
    for name in [
        "dofile",
        "loadfile",
        "load",
        "require",
        "collectgarbage",
        "io",
        "os",
        "debug",
        "package",
    ] {
        globals.set(name, Value::Nil).map_err(|_| {
            invalid(
                "ASTRA_EMU_VFS_PATCH_SANDBOX",
                "trusted private profile sandbox could not be installed",
            )
        })?;
    }
    let astra = lua.create_table().map_err(lua_error)?;
    let family = lua.create_table().map_err(lua_error)?;
    let registrations = Arc::new(Mutex::new(Vec::<Registration>::new()));
    let sink = registrations.clone();
    family
        .set(
            "register_private_profile",
            lua.create_function(move |_, descriptor: Table| {
                let profile_id: String = descriptor.get("id")?;
                let schema_id: String = descriptor.get("schema")?;
                let payload: mlua::Buffer = descriptor.get("payload")?;
                if !safe_symbol(&profile_id)
                    || !safe_symbol(&schema_id)
                    || payload.is_empty()
                    || payload.len() > MAX_PROFILE_BYTES
                {
                    return Err(mlua::Error::runtime(
                        "ASTRA_EMU_VFS_PRIVATE_PROFILE_DESCRIPTOR",
                    ));
                }
                let mut guard = sink
                    .lock()
                    .map_err(|_| mlua::Error::runtime("ASTRA_EMU_VFS_PRIVATE_PROFILE_LOCK"))?;
                if !guard.is_empty() {
                    return Err(mlua::Error::runtime(
                        "ASTRA_EMU_VFS_PRIVATE_PROFILE_DUPLICATE",
                    ));
                }
                guard.push(Registration {
                    profile_id,
                    schema_id,
                    payload: payload.to_vec(),
                });
                Ok(())
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;
    astra.set("family", family).map_err(lua_error)?;
    globals.set("astra", astra).map_err(lua_error)?;
    drop(globals);
    install_budget(&lua);
    lua.load(source_text)
        .set_name("astraemu.patch.luau")
        .exec()
        .map_err(|error| {
            tracing::error!(
                event = "astra_emu_vfs_private_profile_failed",
                cause = sanitized_lua_error(&error)
            );
            invalid(
                "ASTRA_EMU_VFS_PATCH_EXEC",
                "trusted private profile patch failed",
            )
        })?;
    lua.remove_interrupt();
    let registration = registrations
        .lock()
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_VFS_PRIVATE_PROFILE_LOCK",
                "private profile registry is poisoned",
            )
        })?
        .pop()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_VFS_PRIVATE_PROFILE_MISSING",
                "trusted patch did not register a private profile",
            )
        })?;
    if registration.profile_id != expected_profile_id
        || registration.schema_id != expected_schema_id
    {
        return Err(invalid(
            "ASTRA_EMU_VFS_PRIVATE_PROFILE_MISMATCH",
            "registered private profile identity does not match the mount profile",
        ));
    }
    Ok(TrustedPrivateProfile {
        profile_id: registration.profile_id,
        schema_hash: Hash256::from_sha256(registration.schema_id.as_bytes()),
        schema_id: registration.schema_id,
        patch_hash: Hash256::from_sha256(&source),
        payload_hash: Hash256::from_sha256(&registration.payload),
        payload: registration.payload.into(),
    })
}

fn install_budget(lua: &Lua) {
    let started = Instant::now();
    let count = AtomicU64::new(0);
    lua.set_interrupt(move |_| {
        if count.fetch_add(1, Ordering::Relaxed) >= INSTRUCTION_LIMIT {
            return Err(mlua::Error::runtime(
                "ASTRA_EMU_VFS_PATCH_INSTRUCTION_LIMIT",
            ));
        }
        if started.elapsed() > WALL_CLOCK_LIMIT {
            return Err(mlua::Error::runtime("ASTRA_EMU_VFS_PATCH_TIMEOUT"));
        }
        Ok(VmState::Continue)
    });
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn sanitized_lua_error(error: &mlua::Error) -> &'static str {
    match error {
        mlua::Error::MemoryError(_) => "memory",
        mlua::Error::SyntaxError { .. } => "syntax",
        mlua::Error::RuntimeError(message) if message.contains("TIMEOUT") => "timeout",
        mlua::Error::RuntimeError(message) if message.contains("INSTRUCTION_LIMIT") => {
            "instruction_limit"
        }
        mlua::Error::RuntimeError(_) => "runtime",
        mlua::Error::CallbackError { cause, .. } => sanitized_lua_error(cause),
        _ => "lua_api",
    }
}

fn lua_error(_: mlua::Error) -> LegacyCoreError {
    invalid(
        "ASTRA_EMU_VFS_PATCH_API",
        "trusted private profile API initialization failed",
    )
}
fn invalid(code: &'static str, message: &'static str) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_profile_is_data_only_and_duplicate_registration_blocks() {
        let root = tempfile::tempdir().unwrap();
        let patch = root.path().join("patch.luau");
        std::fs::write(&patch, "astra.family.register_private_profile({id='fixture',schema='fixture.private.v1',payload=buffer.fromstring('secret')})").unwrap();
        let profile = load_private_profile(&patch, "fixture", "fixture.private.v1").unwrap();
        assert_eq!(profile.payload(), b"secret");
        std::fs::write(&patch, "local p={id='fixture',schema='fixture.private.v1',payload=buffer.fromstring('x')} astra.family.register_private_profile(p) astra.family.register_private_profile(p)").unwrap();
        assert_eq!(
            load_private_profile(&patch, "fixture", "fixture.private.v1")
                .unwrap_err()
                .code(),
            "ASTRA_EMU_VFS_PATCH_EXEC"
        );
    }
}
