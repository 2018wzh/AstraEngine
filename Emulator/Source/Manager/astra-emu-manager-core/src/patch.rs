use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use mlua::{Lua, LuaOptions, StdLib, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PatchEffectIntent {
    pub kind: String,
    pub target: String,
    pub payload_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PatchDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchExecution {
    pub intents: Vec<PatchEffectIntent>,
    pub overlays: BTreeMap<String, Vec<u8>>,
    pub host_actions: Vec<PatchHostAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchHostAction {
    DecodeTransform {
        path: String,
        bytes: Vec<u8>,
    },
    TextHook {
        target_hash: String,
        replacement: String,
    },
    MediaHook {
        target_hash: String,
        replacement_uri: String,
    },
    DeterministicEffect {
        target: String,
        payload: Vec<u8>,
    },
}

pub trait PatchVfsReader: Send + Sync {
    fn read(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, PatchDiagnostic>;
}

pub struct TrustedPatchRuntime {
    instruction_budget: u32,
    memory_budget_bytes: usize,
}

#[derive(Clone, Default)]
pub struct PatchContext {
    pub files: BTreeMap<String, Vec<u8>>,
    pub reader: Option<Arc<dyn PatchVfsReader>>,
}

impl TrustedPatchRuntime {
    pub fn new(instruction_budget: u32) -> Result<Self, PatchDiagnostic> {
        if instruction_budget == 0 || instruction_budget > 10_000_000 {
            return Err(PatchDiagnostic {
                code: "ASTRA_EMU_PATCH_BUDGET".into(),
                message: "instruction budget is outside the supported bound".into(),
            });
        }
        Ok(Self {
            instruction_budget,
            memory_budget_bytes: 64 * 1024 * 1024,
        })
    }

    pub fn evaluate(
        &self,
        source: &str,
        context: &PatchContext,
    ) -> Result<PatchExecution, PatchDiagnostic> {
        if source.len() > 256 * 1024 {
            return Err(PatchDiagnostic {
                code: "ASTRA_EMU_PATCH_SOURCE_BOUNDS".into(),
                message: "patch source exceeds 256 KiB".into(),
            });
        }
        // Each evaluation owns a fresh VM. This prevents functions, threads, userdata, globals,
        // metatable changes, or a previous case's host API from crossing an evaluation boundary.
        let lua = create_sandbox(self.memory_budget_bytes)?;
        let files = Arc::new(context.files.clone());
        let reader = context.reader.clone();
        let intents = Arc::new(Mutex::new(Vec::new()));
        let overlays = Arc::new(Mutex::new(BTreeMap::<String, Vec<u8>>::new()));
        let host_actions = Arc::new(Mutex::new(Vec::<PatchHostAction>::new()));
        let output_bytes = Arc::new(Mutex::new(0usize));
        let astra = lua
            .create_table()
            .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?;
        let read_files = files.clone();
        astra
            .set(
                "read",
                lua.create_function(move |_lua, path: String| {
                    let path = safe_patch_path(&path).map_err(mlua::Error::runtime)?;
                    if let Some(bytes) = read_files.get(&path) {
                        if bytes.len() > 16 * 1024 * 1024 {
                            return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_VFS_FILE_BOUNDS"));
                        }
                        return Ok(bytes.clone());
                    }
                    reader
                        .as_ref()
                        .ok_or_else(|| mlua::Error::runtime("ASTRA_EMU_PATCH_VFS_NOT_FOUND"))?
                        .read(&path, 16 * 1024 * 1024)
                        .map_err(|diagnostic| mlua::Error::runtime(diagnostic.code))
                })
                .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?,
            )
            .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?;
        let emitted = intents.clone();
        let emitted_actions = host_actions.clone();
        let emitted_bytes = output_bytes.clone();
        astra
            .set(
                "emit",
                lua.create_function(
                    move |_lua, (kind, target, payload): (String, String, Vec<u8>)| {
                        if !is_safe_symbol(&kind) || !is_safe_symbol(&target) {
                            return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_INTENT_SYMBOL"));
                        }
                        if !matches!(
                            kind.as_str(),
                            "text_hook" | "media_hook" | "trace" | "deterministic_effect"
                        ) {
                            return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_INTENT_KIND"));
                        }
                        if payload.len() > 16 * 1024 * 1024 {
                            return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_INTENT_BOUNDS"));
                        }
                        let mut guard = emitted
                            .lock()
                            .map_err(|_| mlua::Error::runtime("ASTRA_EMU_PATCH_INTENT_LOCK"))?;
                        if guard.len() >= 4096 {
                            return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_INTENT_COUNT"));
                        }
                        reserve_output_bytes(&emitted_bytes, payload.len())?;
                        let target_hash = blake3::hash(target.as_bytes()).to_hex().to_string();
                        let action = match kind.as_str() {
                            "text_hook" => {
                                if target != "all" && !is_hash(&target) {
                                    return Err(mlua::Error::runtime(
                                        "ASTRA_EMU_PATCH_TEXT_HOOK_TARGET",
                                    ));
                                }
                                let replacement =
                                    String::from_utf8(payload.clone()).map_err(|_| {
                                        mlua::Error::runtime("ASTRA_EMU_PATCH_TEXT_HOOK_UTF8")
                                    })?;
                                if replacement.len() > 64 * 1024 {
                                    return Err(mlua::Error::runtime(
                                        "ASTRA_EMU_PATCH_TEXT_HOOK_BOUNDS",
                                    ));
                                }
                                Some(PatchHostAction::TextHook {
                                    target_hash: target,
                                    replacement,
                                })
                            }
                            "media_hook" => {
                                if !is_hash(&target) {
                                    return Err(mlua::Error::runtime(
                                        "ASTRA_EMU_PATCH_MEDIA_HOOK_TARGET",
                                    ));
                                }
                                let replacement_uri =
                                    String::from_utf8(payload.clone()).map_err(|_| {
                                        mlua::Error::runtime("ASTRA_EMU_PATCH_MEDIA_HOOK_UTF8")
                                    })?;
                                let replacement_uri = safe_patch_path(&replacement_uri)
                                    .map_err(mlua::Error::runtime)?;
                                Some(PatchHostAction::MediaHook {
                                    target_hash: target,
                                    replacement_uri,
                                })
                            }
                            "deterministic_effect" => {
                                if payload.len() > 1024 * 1024 {
                                    return Err(mlua::Error::runtime(
                                        "ASTRA_EMU_PATCH_EFFECT_BOUNDS",
                                    ));
                                }
                                Some(PatchHostAction::DeterministicEffect {
                                    target: target.clone(),
                                    payload: payload.clone(),
                                })
                            }
                            "trace" => None,
                            _ => unreachable!(),
                        };
                        if let Some(action) = action {
                            let mut actions = emitted_actions
                                .lock()
                                .map_err(|_| mlua::Error::runtime("ASTRA_EMU_PATCH_ACTION_LOCK"))?;
                            if actions.len() >= 4096 {
                                return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_ACTION_COUNT"));
                            }
                            actions.push(action);
                        }
                        guard.push(PatchEffectIntent {
                            kind,
                            target: target_hash,
                            payload_hash: blake3::hash(&payload).to_hex().to_string(),
                        });
                        Ok(())
                    },
                )
                .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?,
            )
            .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?;
        let overlay_intents = intents.clone();
        let overlay_output = overlays.clone();
        let overlay_bytes = output_bytes.clone();
        astra
            .set(
                "overlay",
                lua.create_function(move |_lua, (path, payload): (String, Vec<u8>)| {
                    let path = safe_patch_path(&path).map_err(mlua::Error::runtime)?;
                    if payload.len() > 16 * 1024 * 1024 {
                        return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_OVERLAY_BOUNDS"));
                    }
                    reserve_output_bytes(&overlay_bytes, payload.len())?;
                    let payload_hash = blake3::hash(&payload).to_hex().to_string();
                    let mut output = overlay_output
                        .lock()
                        .map_err(|_| mlua::Error::runtime("ASTRA_EMU_PATCH_OVERLAY_LOCK"))?;
                    if output.len() >= 4096 {
                        return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_OVERLAY_COUNT"));
                    }
                    if output.insert(path.clone(), payload).is_some() {
                        return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_OVERLAY_DUPLICATE"));
                    }
                    overlay_intents
                        .lock()
                        .map_err(|_| mlua::Error::runtime("ASTRA_EMU_PATCH_INTENT_LOCK"))?
                        .push(PatchEffectIntent {
                            kind: "overlay".into(),
                            target: path,
                            payload_hash,
                        });
                    Ok(())
                })
                .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?,
            )
            .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?;
        let decode_intents = intents.clone();
        let decode_actions = host_actions.clone();
        let decode_bytes = output_bytes.clone();
        astra
            .set(
                "decode_transform",
                lua.create_function(move |_lua, (path, payload): (String, Vec<u8>)| {
                    let path = safe_patch_path(&path).map_err(mlua::Error::runtime)?;
                    if payload.len() > 16 * 1024 * 1024 {
                        return Err(mlua::Error::runtime(
                            "ASTRA_EMU_PATCH_DECODE_TRANSFORM_BOUNDS",
                        ));
                    }
                    reserve_output_bytes(&decode_bytes, payload.len())?;
                    let path_hash = blake3::hash(path.as_bytes()).to_hex().to_string();
                    let payload_hash = blake3::hash(&payload).to_hex().to_string();
                    let mut actions = decode_actions
                        .lock()
                        .map_err(|_| mlua::Error::runtime("ASTRA_EMU_PATCH_ACTION_LOCK"))?;
                    if actions.len() >= 4096
                        || actions.iter().any(|action| {
                            matches!(action, PatchHostAction::DecodeTransform { path: existing, .. } if existing.eq_ignore_ascii_case(&path))
                        })
                    {
                        return Err(mlua::Error::runtime(
                            "ASTRA_EMU_PATCH_DECODE_TRANSFORM_DUPLICATE",
                        ));
                    }
                    actions.push(PatchHostAction::DecodeTransform {
                        path,
                        bytes: payload,
                    });
                    decode_intents
                        .lock()
                        .map_err(|_| mlua::Error::runtime("ASTRA_EMU_PATCH_INTENT_LOCK"))?
                        .push(PatchEffectIntent {
                            kind: "decode_transform".into(),
                            target: path_hash,
                            payload_hash,
                        });
                    Ok(())
                })
                .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?,
            )
            .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?;
        astra.set_readonly(true);
        lua.globals()
            .set("astra", astra)
            .map_err(|error| diagnostic("ASTRA_EMU_PATCH_API", error))?;

        let remaining =
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(self.instruction_budget));
        let hook_remaining = remaining.clone();
        lua.set_interrupt(move |_lua| {
            let previous = hook_remaining.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            if previous <= 1 {
                return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_INSTRUCTION_BUDGET"));
            }
            Ok(mlua::VmState::Continue)
        });
        let value: Value = lua
            .load(source)
            .set_name("trusted_patch")
            .eval()
            .map_err(|error| diagnostic("ASTRA_EMU_PATCH_EXECUTION", error))?;
        lua.remove_interrupt();
        if !matches!(value, Value::Nil) {
            return Err(PatchDiagnostic {
                code: "ASTRA_EMU_PATCH_RETURN_TYPE".into(),
                message:
                    "trusted patch chunks must return nil and emit typed intents through host APIs"
                        .into(),
            });
        }
        let intents = intents
            .lock()
            .map_err(|_| PatchDiagnostic {
                code: "ASTRA_EMU_PATCH_INTENT_LOCK".into(),
                message: "patch intent lock is poisoned".into(),
            })?
            .clone();
        let overlays = overlays
            .lock()
            .map_err(|_| PatchDiagnostic {
                code: "ASTRA_EMU_PATCH_OVERLAY_LOCK".into(),
                message: "patch overlay lock is poisoned".into(),
            })?
            .clone();
        let host_actions = host_actions
            .lock()
            .map_err(|_| PatchDiagnostic {
                code: "ASTRA_EMU_PATCH_ACTION_LOCK".into(),
                message: "patch action lock is poisoned".into(),
            })?
            .clone();
        Ok(PatchExecution {
            intents,
            overlays,
            host_actions,
        })
    }
}

fn reserve_output_bytes(total: &Mutex<usize>, additional: usize) -> mlua::Result<()> {
    let mut total = total
        .lock()
        .map_err(|_| mlua::Error::runtime("ASTRA_EMU_PATCH_OUTPUT_LOCK"))?;
    let next = total
        .checked_add(additional)
        .ok_or_else(|| mlua::Error::runtime("ASTRA_EMU_PATCH_OUTPUT_BOUNDS"))?;
    if next > 64 * 1024 * 1024 {
        return Err(mlua::Error::runtime("ASTRA_EMU_PATCH_OUTPUT_BOUNDS"));
    }
    *total = next;
    Ok(())
}

fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn create_sandbox(memory_budget_bytes: usize) -> Result<Lua, PatchDiagnostic> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
        LuaOptions::new(),
    )
    .map_err(|error| diagnostic("ASTRA_EMU_PATCH_LUA_CREATE", error))?;
    lua.set_memory_limit(memory_budget_bytes)
        .map_err(|error| diagnostic("ASTRA_EMU_PATCH_MEMORY_BUDGET", error))?;
    let globals = lua.globals();
    for forbidden in [
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
        globals
            .set(forbidden, Value::Nil)
            .map_err(|error| diagnostic("ASTRA_EMU_PATCH_SANDBOX", error))?;
    }
    drop(globals);
    Ok(lua)
}

fn safe_patch_path(value: &str) -> Result<String, &'static str> {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty()
        || normalized.len() > 4096
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("ASTRA_EMU_PATCH_VFS_PATH");
    }
    Ok(normalized)
}

fn is_safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn diagnostic(code: &str, _error: impl std::fmt::Display) -> PatchDiagnostic {
    PatchDiagnostic {
        code: code.into(),
        message: "trusted patch execution failed; inspect the diagnostic code".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_has_no_filesystem_network_system_or_loader_capabilities() {
        let runtime = TrustedPatchRuntime::new(10_000).unwrap();
        for name in [
            "io", "os", "debug", "package", "require", "loadfile", "dofile",
        ] {
            let source = format!("assert({name} == nil); return nil");
            assert!(runtime.evaluate(&source, &PatchContext::default()).is_ok());
        }
    }

    #[test]
    fn evaluations_do_not_share_lua_globals() {
        let runtime = TrustedPatchRuntime::new(10_000).unwrap();
        runtime
            .evaluate(
                "previous_case_value = 42; return nil",
                &PatchContext::default(),
            )
            .unwrap();
        runtime
            .evaluate(
                "assert(previous_case_value == nil); return nil",
                &PatchContext::default(),
            )
            .unwrap();
    }

    #[test]
    fn traversal_and_instruction_exhaustion_are_stable_diagnostics() {
        let runtime = TrustedPatchRuntime::new(10).unwrap();
        let traversal = runtime
            .evaluate("astra.read('../secret')", &PatchContext::default())
            .unwrap_err();
        assert_eq!(traversal.code, "ASTRA_EMU_PATCH_EXECUTION");
        assert!(!traversal.message.contains("secret"));
        let exhausted = runtime
            .evaluate("while true do end", &PatchContext::default())
            .unwrap_err();
        assert_eq!(exhausted.code, "ASTRA_EMU_PATCH_EXECUTION");
    }

    #[test]
    fn overlay_bytes_are_ephemeral_while_trace_only_exposes_hash() {
        let runtime = TrustedPatchRuntime::new(10_000).unwrap();
        let execution = runtime
            .evaluate(
                "astra.overlay('script.bin', {1, 2, 3, 4}); return nil",
                &PatchContext::default(),
            )
            .unwrap();
        assert_eq!(execution.overlays["script.bin"], [1, 2, 3, 4]);
        assert_eq!(execution.intents.len(), 1);
        assert_eq!(execution.intents[0].kind, "overlay");
        assert!(!execution.intents[0].payload_hash.is_empty());
        assert!(!execution.intents[0].payload_hash.contains("1, 2, 3, 4"));
    }

    #[test]
    fn decode_text_media_and_deterministic_actions_remain_private_and_typed() {
        let runtime = TrustedPatchRuntime::new(100_000).unwrap();
        let target_hash = "a".repeat(64);
        let source = format!(
            "astra.decode_transform('script.bin', {{1,2,3}}); \
             astra.emit('text_hook', 'all', {{82,101,112,108,97,99,101,100}}); \
             astra.emit('media_hook', '{target_hash}', {{97,117,100,105,111,47,110,101,119,46,111,103,103}}); \
             astra.emit('deterministic_effect', 'event.patch_ready', {{9,8,7}}); return nil"
        );
        let execution = runtime.evaluate(&source, &PatchContext::default()).unwrap();
        assert_eq!(execution.host_actions.len(), 4);
        assert!(matches!(
            &execution.host_actions[0],
            PatchHostAction::DecodeTransform { path, bytes }
                if path == "script.bin" && bytes == &[1, 2, 3]
        ));
        assert!(matches!(
            &execution.host_actions[1],
            PatchHostAction::TextHook { target_hash, replacement }
                if target_hash == "all" && replacement == "Replaced"
        ));
        assert!(matches!(
            &execution.host_actions[2],
            PatchHostAction::MediaHook { target_hash: actual, replacement_uri }
                if actual == &target_hash && replacement_uri == "audio/new.ogg"
        ));
        assert!(matches!(
            &execution.host_actions[3],
            PatchHostAction::DeterministicEffect { target, payload }
                if target == "event.patch_ready" && payload == &[9, 8, 7]
        ));
        assert!(execution
            .intents
            .iter()
            .all(|intent| intent.target == "all" || is_hash(&intent.target)));
        let debug = format!("{:?}", execution.intents);
        assert!(!debug.contains("script.bin"));
        assert!(!debug.contains("audio/new.ogg"));
        assert!(!debug.contains("Replaced"));
    }
}
