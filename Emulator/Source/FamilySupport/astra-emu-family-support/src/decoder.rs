use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use astra_core::Hash256;
use astra_emu_family_core::{
    validate_decrypt_output, validate_decrypt_request, LegacyCoreError, LegacyDecryptPhase,
    LegacyDecryptProvider, LegacyDecryptRequest,
};
use blowfish::{
    cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit},
    Blowfish,
};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use mlua::{Function, Lua, LuaOptions, RegistryKey, StdLib, Table, Value, VmState};
use rc4::{Rc4, StreamCipher};

pub const DECODER_CAPABILITY: &str = "astra.vfs.decrypt.v2";
pub const MAX_DECODER_BATCH_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_DECODER_BATCH_ENTRIES: usize = 64;
pub const DECODER_CHUNK_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct DecoderLimits {
    pub instruction_budget: u64,
    pub memory_bytes: usize,
    pub output_bytes: usize,
    pub wall_clock: Duration,
}

impl Default for DecoderLimits {
    fn default() -> Self {
        Self {
            instruction_budget: 10_000_000,
            memory_bytes: 128 * 1024 * 1024,
            output_bytes: MAX_DECODER_BATCH_BYTES,
            wall_clock: Duration::from_secs(5),
        }
    }
}

struct DecoderRegistration {
    id: String,
    descriptor_schema_id: String,
    descriptor_schema_hash: Hash256,
    private_profile_hash: Hash256,
    decode_index: RegistryKey,
    decode_entries: RegistryKey,
}

struct LuaDecoderRuntime {
    lua: Lua,
    decoders: BTreeMap<String, DecoderRegistration>,
    limits: DecoderLimits,
}

impl LuaDecoderRuntime {
    fn load(
        source: &str,
        required_capabilities: &BTreeSet<String>,
        limits: DecoderLimits,
    ) -> Result<Self, LegacyCoreError> {
        if source.len() > 256 * 1024
            || limits.instruction_budget == 0
            || limits.output_bytes > MAX_DECODER_BATCH_BYTES
        {
            return Err(error(
                "ASTRA_EMU_DECODER_LIMITS",
                "decoder source or limits are outside the supported bounds",
            ));
        }
        if !required_capabilities.contains(DECODER_CAPABILITY) {
            return Err(error(
                "ASTRA_EMU_DECODER_CAPABILITY",
                "trusted profile does not grant the VFS decoder capability",
            ));
        }
        let lua = Lua::new_with(
            StdLib::TABLE
                | StdLib::STRING
                | StdLib::MATH
                | StdLib::UTF8
                | StdLib::BIT
                | StdLib::BUFFER,
            LuaOptions::new(),
        )
        .map_err(|_| error("ASTRA_EMU_DECODER_LUA_CREATE", "Luau VM creation failed"))?;
        lua.set_memory_limit(limits.memory_bytes).map_err(|_| {
            error(
                "ASTRA_EMU_DECODER_MEMORY",
                "Luau memory limit cannot be installed",
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
                error(
                    "ASTRA_EMU_DECODER_SANDBOX",
                    "Luau sandbox initialization failed",
                )
            })?;
        }
        let astra = lua.create_table().map_err(lua_api_error)?;
        let vfs = lua.create_table().map_err(lua_api_error)?;
        let registrations = Arc::new(Mutex::new(Vec::<DecoderRegistration>::new()));
        let registration_sink = registrations.clone();
        vfs.set(
            "register_decoder",
            lua.create_function(move |lua, descriptor: Table| {
                let id: String = descriptor.get("id")?;
                if !safe_symbol(&id) {
                    return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_ID"));
                }
                let capabilities: Vec<String> = descriptor.get("capabilities")?;
                if capabilities != [DECODER_CAPABILITY.to_owned()] {
                    return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_CAPABILITY"));
                }
                let descriptor_schema_id: String = descriptor.get("descriptor_schema")?;
                if !safe_symbol(&descriptor_schema_id) {
                    return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_DESCRIPTOR_SCHEMA"));
                }
                let private_profile_hash: mlua::Buffer = descriptor.get("private_profile_hash")?;
                let private_profile_hash: [u8; 32] = private_profile_hash
                    .to_vec()
                    .try_into()
                    .map_err(|_| mlua::Error::runtime("ASTRA_EMU_DECODER_PRIVATE_PROFILE_HASH"))?;
                let decode_index: Function = descriptor.get("decode_index")?;
                let decode_entries: Function = descriptor.get("decode_entries")?;
                let mut guard = registration_sink
                    .lock()
                    .map_err(|_| mlua::Error::runtime("ASTRA_EMU_DECODER_REGISTRY_LOCK"))?;
                if guard.iter().any(|registered| registered.id == id) {
                    return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_DUPLICATE"));
                }
                guard.push(DecoderRegistration {
                    id,
                    descriptor_schema_hash: Hash256::from_sha256(descriptor_schema_id.as_bytes()),
                    descriptor_schema_id,
                    private_profile_hash: Hash256::from_bytes(private_profile_hash),
                    decode_index: lua.create_registry_value(decode_index)?,
                    decode_entries: lua.create_registry_value(decode_entries)?,
                });
                Ok(())
            })
            .map_err(lua_api_error)?,
        )
        .map_err(lua_api_error)?;
        install_intrinsics(&lua, &vfs, limits)?;
        astra.set("vfs", vfs).map_err(lua_api_error)?;
        globals.set("astra", astra).map_err(lua_api_error)?;
        drop(globals);
        install_budget(&lua, limits);
        lua.load(source)
            .set_name("astraemu.patch.luau")
            .exec()
            .map_err(|_| {
                error(
                    "ASTRA_EMU_DECODER_PATCH_EXEC",
                    "trusted decoder patch failed",
                )
            })?;
        lua.remove_interrupt();
        let mut decoders = BTreeMap::new();
        let registrations = registrations
            .lock()
            .map_err(|_| error("ASTRA_EMU_DECODER_REGISTRY", "decoder registry is poisoned"))?
            .drain(..)
            .collect::<Vec<_>>();
        if registrations.is_empty() {
            return Err(error(
                "ASTRA_EMU_DECODER_MISSING",
                "trusted patch did not register a decoder",
            ));
        }
        for registration in registrations {
            decoders.insert(registration.id.clone(), registration);
        }
        Ok(Self {
            lua,
            decoders,
            limits,
        })
    }
}

#[derive(Clone)]
struct DecoderIdentity {
    descriptor_schema_id: String,
    descriptor_schema_hash: Hash256,
    private_profile_hash: Hash256,
}

struct OwnedDecryptRequest {
    phase: LegacyDecryptPhase,
    descriptors: Vec<Vec<u8>>,
    transport: astra_emu_family_core::LegacyDecryptTransport,
    bytes: Vec<u8>,
}

enum DecoderWorkerCommand {
    Decrypt {
        id: String,
        request: OwnedDecryptRequest,
        response: mpsc::SyncSender<Result<Vec<u8>, LegacyCoreError>>,
    },
    Shutdown,
}

struct DecoderWorker {
    sender: mpsc::Sender<DecoderWorkerCommand>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for DecoderWorker {
    fn drop(&mut self) {
        let _ = self.sender.send(DecoderWorkerCommand::Shutdown);
        if let Ok(join) = self.join.get_mut() {
            if let Some(join) = join.take() {
                let _ = join.join();
            }
        }
    }
}

pub struct TrustedDecoderSession {
    patch_hash: Hash256,
    decoders: BTreeMap<String, DecoderIdentity>,
    worker: DecoderWorker,
}

impl TrustedDecoderSession {
    pub fn load(
        source: &str,
        required_capabilities: &BTreeSet<String>,
        limits: DecoderLimits,
    ) -> Result<Self, LegacyCoreError> {
        let patch_hash = Hash256::from_sha256(source.as_bytes());
        let (commands, receiver) = mpsc::channel();
        let (initialized, initialization) = mpsc::sync_channel(1);
        let source = source.to_owned();
        let capabilities = required_capabilities.clone();
        let join = thread::Builder::new()
            .name("astra-vfs-luau-decoder".into())
            .spawn(move || {
                let runtime = match LuaDecoderRuntime::load(&source, &capabilities, limits) {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = initialized.send(Err(error));
                        return;
                    }
                };
                let identities = runtime
                    .decoders
                    .iter()
                    .map(|(id, registration)| {
                        (
                            id.clone(),
                            DecoderIdentity {
                                descriptor_schema_id: registration.descriptor_schema_id.clone(),
                                descriptor_schema_hash: registration.descriptor_schema_hash,
                                private_profile_hash: registration.private_profile_hash,
                            },
                        )
                    })
                    .collect();
                if initialized.send(Ok(identities)).is_err() {
                    return;
                }
                while let Ok(command) = receiver.recv() {
                    match command {
                        DecoderWorkerCommand::Decrypt {
                            id,
                            request,
                            response,
                        } => {
                            let _ = response.send(runtime.decrypt(&id, request));
                        }
                        DecoderWorkerCommand::Shutdown => break,
                    }
                }
            })
            .map_err(|_| {
                error(
                    "ASTRA_EMU_DECODER_WORKER_CREATE",
                    "trusted decoder worker could not be created",
                )
            })?;
        let decoders = initialization.recv().map_err(|_| {
            error(
                "ASTRA_EMU_DECODER_WORKER_INIT",
                "trusted decoder worker stopped during initialization",
            )
        })??;
        Ok(Self {
            patch_hash,
            decoders,
            worker: DecoderWorker {
                sender: commands,
                join: Mutex::new(Some(join)),
            },
        })
    }

    pub fn decoder(self: &Arc<Self>, id: &str) -> Result<SessionDecoder, LegacyCoreError> {
        if !self.decoders.contains_key(id) {
            return Err(error(
                "ASTRA_EMU_DECODER_NOT_FOUND",
                "trusted decoder id is not registered",
            ));
        }
        Ok(SessionDecoder {
            session: self.clone(),
            id: id.to_owned(),
        })
    }

    pub fn patch_hash(&self) -> Hash256 {
        self.patch_hash
    }
}

pub struct SessionDecoder {
    session: Arc<TrustedDecoderSession>,
    id: String,
}

impl LegacyDecryptProvider for SessionDecoder {
    fn provider_id(&self) -> &str {
        &self.id
    }
    fn private_profile_hash(&self) -> Hash256 {
        self.session.decoders[&self.id].private_profile_hash
    }
    fn descriptor_schema_id(&self) -> &str {
        &self.session.decoders[&self.id].descriptor_schema_id
    }
    fn descriptor_schema_hash(&self) -> Hash256 {
        self.session.decoders[&self.id].descriptor_schema_hash
    }

    fn decrypt(&self, request: LegacyDecryptRequest<'_>) -> Result<Vec<u8>, LegacyCoreError> {
        validate_decrypt_request(self, &request)?;
        let owned = OwnedDecryptRequest {
            phase: request.phase,
            descriptors: request
                .descriptors
                .iter()
                .map(|descriptor| descriptor.payload.clone())
                .collect(),
            transport: request.transport,
            bytes: request.bytes.to_vec(),
        };
        let (response, result) = mpsc::sync_channel(1);
        self.session
            .worker
            .sender
            .send(DecoderWorkerCommand::Decrypt {
                id: self.id.clone(),
                request: owned,
                response,
            })
            .map_err(|_| {
                error(
                    "ASTRA_EMU_DECODER_WORKER_STOPPED",
                    "trusted decoder worker is not available",
                )
            })?;
        let output = result.recv().map_err(|_| {
            error(
                "ASTRA_EMU_DECODER_WORKER_STOPPED",
                "trusted decoder worker stopped before returning output",
            )
        })??;
        validate_decrypt_output(&request, &output)?;
        Ok(output)
    }
}

impl LuaDecoderRuntime {
    fn decrypt(&self, id: &str, request: OwnedDecryptRequest) -> Result<Vec<u8>, LegacyCoreError> {
        let registration = self.decoders.get(id).ok_or_else(|| {
            error(
                "ASTRA_EMU_DECODER_NOT_FOUND",
                "trusted decoder id is not registered",
            )
        })?;
        let key = match request.phase {
            LegacyDecryptPhase::Index => &registration.decode_index,
            LegacyDecryptPhase::Entry => &registration.decode_entries,
        };
        let function: Function = self.lua.registry_value(key).map_err(lua_call_error)?;
        let descriptors = self.lua.create_table().map_err(lua_call_error)?;
        for (index, descriptor) in request.descriptors.iter().enumerate() {
            descriptors
                .set(
                    index + 1,
                    self.lua.create_buffer(descriptor).map_err(lua_call_error)?,
                )
                .map_err(lua_call_error)?;
        }
        let transport = self.lua.create_table().map_err(lua_call_error)?;
        transport
            .set("chunk_offset", request.transport.chunk_offset)
            .map_err(lua_call_error)?;
        transport
            .set("total_size", request.transport.total_size)
            .map_err(lua_call_error)?;
        transport
            .set("batch_index", request.transport.batch_index)
            .map_err(lua_call_error)?;
        transport
            .set("input_bound", request.transport.input_bound)
            .map_err(lua_call_error)?;
        transport
            .set("output_bound", request.transport.output_bound)
            .map_err(lua_call_error)?;
        let output = self.call(
            function,
            (
                self.lua
                    .create_buffer(request.bytes)
                    .map_err(lua_call_error)?,
                descriptors,
                transport,
            ),
        )?;
        Ok(output)
    }

    fn call<A: mlua::IntoLuaMulti>(
        &self,
        function: Function,
        arguments: A,
    ) -> Result<Vec<u8>, LegacyCoreError> {
        install_budget(&self.lua, self.limits);
        let value: Value = function.call(arguments).map_err(lua_call_error)?;
        self.lua.remove_interrupt();
        let bytes = match value {
            Value::Buffer(buffer) => buffer.to_vec(),
            _ => {
                return Err(error(
                    "ASTRA_EMU_DECODER_OUTPUT_TYPE",
                    "decoder callback must return a buffer",
                ))
            }
        };
        if bytes.len() > self.limits.output_bytes {
            return Err(error(
                "ASTRA_EMU_DECODER_OUTPUT_LIMIT",
                "decoder callback output exceeds its limit",
            ));
        }
        Ok(bytes)
    }
}

fn install_budget(lua: &Lua, limits: DecoderLimits) {
    let started = Instant::now();
    let count = Arc::new(AtomicU64::new(0));
    lua.set_interrupt(move |_| {
        if count.fetch_add(1, Ordering::Relaxed) >= limits.instruction_budget {
            return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_INSTRUCTION_LIMIT"));
        }
        if started.elapsed() > limits.wall_clock {
            return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_TIMEOUT"));
        }
        Ok(VmState::Continue)
    });
}

fn install_intrinsics(
    lua: &Lua,
    vfs: &Table,
    limits: DecoderLimits,
) -> Result<(), LegacyCoreError> {
    vfs.set(
        "crc32",
        lua.create_function(|_, bytes: mlua::Buffer| Ok(crc32fast::hash(&bytes.to_vec())))
            .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    vfs.set(
        "sha256",
        lua.create_function(|lua, bytes: mlua::Buffer| {
            lua.create_buffer(Hash256::from_sha256(&bytes.to_vec()).as_bytes())
        })
        .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    vfs.set(
        "xor_u32",
        lua.create_function(|lua, (bytes, key): (mlua::Buffer, u32)| {
            let mut out = bytes.to_vec();
            let key = key.to_le_bytes();
            for (i, b) in out.iter_mut().enumerate() {
                *b ^= key[i % 4];
            }
            lua.create_buffer(out)
        })
        .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    vfs.set(
        "blowfish",
        lua.create_function(
            |lua, (bytes, key, decrypt): (mlua::Buffer, mlua::Buffer, bool)| {
                if !bytes.len().is_multiple_of(8) {
                    return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_BLOWFISH_ALIGNMENT"));
                }
                let cipher: Blowfish = Blowfish::new_from_slice(&key.to_vec())
                    .map_err(|_| mlua::Error::runtime("ASTRA_EMU_DECODER_BLOWFISH_KEY"))?;
                let mut out = bytes.to_vec();
                for chunk in out.chunks_exact_mut(8) {
                    chunk[..4].reverse();
                    chunk[4..].reverse();
                    let block: &mut [u8; 8] =
                        chunk.try_into().expect("chunks_exact_mut yields 8 bytes");
                    if decrypt {
                        cipher.decrypt_block(block.into())
                    } else {
                        cipher.encrypt_block(block.into())
                    }
                    block[..4].reverse();
                    block[4..].reverse();
                }
                lua.create_buffer(out)
            },
        )
        .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    vfs.set(
        "rc4",
        lua.create_function(
            |lua, (bytes, key, skip): (mlua::Buffer, mlua::Buffer, u32)| {
                if skip as usize > MAX_DECODER_BATCH_BYTES {
                    return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_RC4_SKIP"));
                }
                let mut cipher = Rc4::new_from_slice(&key.to_vec())
                    .map_err(|_| mlua::Error::runtime("ASTRA_EMU_DECODER_RC4_KEY"))?;
                let mut remaining = skip as usize;
                let mut discarded = vec![0; DECODER_CHUNK_BYTES];
                while remaining > 0 {
                    let length = remaining.min(discarded.len());
                    cipher.apply_keystream(&mut discarded[..length]);
                    discarded[..length].fill(0);
                    remaining -= length;
                }
                let mut out = bytes.to_vec();
                cipher.apply_keystream(&mut out);
                lua.create_buffer(out)
            },
        )
        .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    vfs.set(
        "zlib_decode",
        lua.create_function(move |lua, bytes: mlua::Buffer| {
            let source = bytes.to_vec();
            let mut out = Vec::new();
            ZlibDecoder::new(source.as_slice())
                .take((limits.output_bytes as u64).saturating_add(1))
                .read_to_end(&mut out)
                .map_err(mlua::Error::external)?;
            if out.len() > limits.output_bytes {
                return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_ZLIB_OUTPUT_LIMIT"));
            }
            lua.create_buffer(out)
        })
        .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    vfs.set(
        "zlib_encode",
        lua.create_function(|lua, bytes: mlua::Buffer| {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(&bytes.to_vec())
                .map_err(mlua::Error::external)?;
            lua.create_buffer(encoder.finish().map_err(mlua::Error::external)?)
        })
        .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    vfs.set(
        "slice",
        lua.create_function(|lua, (bytes, offset, length): (mlua::Buffer, u32, u32)| {
            let source = bytes.to_vec();
            let end = (offset as usize)
                .checked_add(length as usize)
                .ok_or_else(|| mlua::Error::runtime("ASTRA_EMU_DECODER_SLICE_BOUNDS"))?;
            if end > source.len() || length as usize > DECODER_CHUNK_BYTES {
                return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_SLICE_BOUNDS"));
            }
            lua.create_buffer(&source[offset as usize..end])
        })
        .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    vfs.set(
        "write",
        lua.create_function(
            |lua, (bytes, offset, replacement): (mlua::Buffer, u32, mlua::Buffer)| {
                let mut output = bytes.to_vec();
                let replacement = replacement.to_vec();
                let end = (offset as usize)
                    .checked_add(replacement.len())
                    .ok_or_else(|| mlua::Error::runtime("ASTRA_EMU_DECODER_WRITE_BOUNDS"))?;
                if output.len() > DECODER_CHUNK_BYTES || end > output.len() {
                    return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_WRITE_BOUNDS"));
                }
                output[offset as usize..end].copy_from_slice(&replacement);
                lua.create_buffer(output)
            },
        )
        .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    Ok(())
}

fn safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
fn error(code: &'static str, message: impl Into<String>) -> LegacyCoreError {
    LegacyCoreError::invalid(code, message)
}
fn lua_api_error(_: mlua::Error) -> LegacyCoreError {
    error(
        "ASTRA_EMU_DECODER_API",
        "trusted decoder API initialization failed",
    )
}
fn lua_call_error(lua_error: mlua::Error) -> LegacyCoreError {
    tracing::error!(
        event = "astra_emu_decoder_callback_failed",
        cause = sanitized_lua_error(&lua_error)
    );
    error(
        "ASTRA_EMU_DECODER_CALLBACK",
        "trusted decoder callback failed",
    )
}

fn sanitized_lua_error(error: &mlua::Error) -> &'static str {
    match error {
        mlua::Error::MemoryError(_) => "memory",
        mlua::Error::SafetyError(_) => "safety",
        mlua::Error::SyntaxError { .. } => "syntax",
        mlua::Error::RuntimeError(message) if message.contains("ASTRA_EMU_DECODER_TIMEOUT") => {
            "timeout"
        }
        mlua::Error::RuntimeError(message)
            if message.contains("ASTRA_EMU_DECODER_INSTRUCTION_LIMIT") =>
        {
            "instruction_limit"
        }
        mlua::Error::RuntimeError(message)
            if message.contains("ASTRA_EMU_DECODER_BLOWFISH_ALIGNMENT") =>
        {
            "blowfish_alignment"
        }
        mlua::Error::RuntimeError(message)
            if message.contains("ASTRA_EMU_DECODER_BLOWFISH_KEY") =>
        {
            "blowfish_key"
        }
        mlua::Error::RuntimeError(_) => "runtime",
        mlua::Error::CallbackError { cause, .. } => sanitized_lua_error(cause),
        _ => "lua_api",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_core::{LegacyDecryptTransport, LegacyOpaqueDescriptor};

    const SOURCE: &str = r#"
        astra.vfs.register_decoder({
            id='copy',
            capabilities={'astra.vfs.decrypt.v2'},
            descriptor_schema='fixture.opaque.v1',
            private_profile_hash=buffer.fromstring(string.rep('\0', 32)),
            decode_index=function(bytes, descriptors, transport) return bytes end,
            decode_entries=function(bytes, descriptors, transport) return bytes end
        })
    "#;

    #[test]
    fn buffer_callback_registers_and_runs() {
        let session = Arc::new(
            TrustedDecoderSession::load(
                SOURCE,
                &BTreeSet::from([DECODER_CAPABILITY.into()]),
                DecoderLimits::default(),
            )
            .unwrap(),
        );
        let decoder = session.decoder("copy").unwrap();
        let descriptor = LegacyOpaqueDescriptor {
            schema_id: "fixture.opaque.v1".into(),
            schema_hash: Hash256::from_sha256(b"fixture.opaque.v1"),
            payload: vec![1],
        };
        assert_eq!(
            decoder
                .decrypt(LegacyDecryptRequest {
                    phase: LegacyDecryptPhase::Index,
                    descriptors: std::slice::from_ref(&descriptor),
                    transport: LegacyDecryptTransport {
                        chunk_offset: 0,
                        total_size: 3,
                        batch_index: 0,
                        input_bound: 3,
                        output_bound: 3,
                    },
                    bytes: b"abc",
                })
                .unwrap(),
            b"abc"
        );
    }

    #[test]
    fn missing_capability_is_blocking() {
        assert_eq!(
            TrustedDecoderSession::load("", &BTreeSet::new(), DecoderLimits::default())
                .err()
                .expect("missing capability must fail")
                .code(),
            "ASTRA_EMU_DECODER_CAPABILITY"
        );
    }
}
