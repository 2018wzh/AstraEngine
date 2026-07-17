use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use astra_core::Hash256;
use astra_emu_family_api::LegacyProviderError;
use astra_emu_minori::{MinoriDecodeService, PazEntryDescriptor};
use blowfish::{
    cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit},
    Blowfish,
};
use encoding_rs::SHIFT_JIS;
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use mlua::{Function, Lua, LuaOptions, RegistryKey, StdLib, Table, Value, VmState};
use rc4::{Rc4, StreamCipher};

pub const DECODER_CAPABILITY: &str = "astra.vfs.decode.v1";
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
    decode_index: RegistryKey,
    decode_entries: RegistryKey,
}

pub struct TrustedDecoderSession {
    lua: Lua,
    patch_hash: Hash256,
    decoders: BTreeMap<String, DecoderRegistration>,
    limits: DecoderLimits,
}

impl TrustedDecoderSession {
    pub fn load(
        source: &str,
        required_capabilities: &BTreeSet<String>,
        limits: DecoderLimits,
    ) -> Result<Self, LegacyProviderError> {
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
                    decode_index: lua.create_registry_value(decode_index)?,
                    decode_entries: lua.create_registry_value(decode_entries)?,
                });
                Ok(())
            })
            .map_err(lua_api_error)?,
        )
        .map_err(lua_api_error)?;
        install_intrinsics(&lua, &vfs)?;
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
            patch_hash: Hash256::from_sha256(source.as_bytes()),
            decoders,
            limits,
        })
    }

    pub fn decoder(self: &Arc<Self>, id: &str) -> Result<SessionDecoder, LegacyProviderError> {
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

impl MinoriDecodeService for SessionDecoder {
    fn decoder_id(&self) -> &str {
        &self.id
    }
    fn patch_hash(&self) -> Hash256 {
        self.session.patch_hash
    }
    fn decode_index(
        &self,
        role: &str,
        version: u8,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, LegacyProviderError> {
        if encrypted.len() > MAX_DECODER_BATCH_BYTES {
            return Err(error(
                "ASTRA_EMU_DECODER_BATCH_BYTES",
                "index batch exceeds 64 MiB",
            ));
        }
        let registration = &self.session.decoders[&self.id];
        let function: Function = self
            .session
            .lua
            .registry_value(&registration.decode_index)
            .map_err(lua_call_error)?;
        let descriptor = self.session.lua.create_table().map_err(lua_call_error)?;
        descriptor.set("role", role).map_err(lua_call_error)?;
        descriptor.set("version", version).map_err(lua_call_error)?;
        self.call(
            function,
            (
                self.session
                    .lua
                    .create_buffer(encrypted)
                    .map_err(lua_call_error)?,
                descriptor,
            ),
        )
    }
    fn decode_entry(
        &self,
        version: u8,
        entry: &PazEntryDescriptor,
        encrypted: &[u8],
    ) -> Result<Vec<u8>, LegacyProviderError> {
        let registration = &self.session.decoders[&self.id];
        let function: Function = self
            .session
            .lua
            .registry_value(&registration.decode_entries)
            .map_err(lua_call_error)?;
        let mut output = Vec::with_capacity(encrypted.len());
        for (chunk_index, chunk) in encrypted.chunks(DECODER_CHUNK_BYTES).enumerate() {
            let chunk_offset = chunk_index
                .checked_mul(DECODER_CHUNK_BYTES)
                .ok_or_else(|| {
                    error(
                        "ASTRA_EMU_DECODER_CHUNK_OFFSET",
                        "decoder chunk offset overflowed",
                    )
                })?;
            let list = self.session.lua.create_table().map_err(lua_call_error)?;
            let descriptor = self.session.lua.create_table().map_err(lua_call_error)?;
            descriptor
                .set("role", entry.archive_role.as_str())
                .map_err(lua_call_error)?;
            descriptor
                .set("entry_id", entry.entry_id.as_str())
                .map_err(lua_call_error)?;
            descriptor
                .set("name", entry.name.as_str())
                .map_err(lua_call_error)?;
            descriptor.set("version", version).map_err(lua_call_error)?;
            descriptor
                .set("unpacked_size", entry.unpacked_size)
                .map_err(lua_call_error)?;
            descriptor
                .set("stored_size", entry.stored_size)
                .map_err(lua_call_error)?;
            descriptor
                .set("total_size", encrypted.len() as u64)
                .map_err(lua_call_error)?;
            descriptor
                .set("chunk_offset", chunk_offset as u64)
                .map_err(lua_call_error)?;
            descriptor
                .set("packed", entry.packed)
                .map_err(lua_call_error)?;
            if let Some(video_key) = &entry.video_key {
                descriptor
                    .set(
                        "video_key",
                        self.session
                            .lua
                            .create_buffer(video_key)
                            .map_err(lua_call_error)?,
                    )
                    .map_err(lua_call_error)?;
            }
            list.set(1, descriptor).map_err(lua_call_error)?;
            let decoded = self.call(
                function.clone(),
                (
                    self.session
                        .lua
                        .create_buffer(chunk)
                        .map_err(lua_call_error)?,
                    list,
                ),
            )?;
            if decoded.len() != chunk.len() {
                return Err(error(
                    "ASTRA_EMU_DECODER_CHUNK_SIZE",
                    "decoder callback changed a chunk length",
                ));
            }
            output.extend_from_slice(&decoded);
        }
        Ok(output)
    }
}

impl SessionDecoder {
    fn call<A: mlua::IntoLuaMulti>(
        &self,
        function: Function,
        arguments: A,
    ) -> Result<Vec<u8>, LegacyProviderError> {
        install_budget(&self.session.lua, self.session.limits);
        let value: Value = function.call(arguments).map_err(lua_call_error)?;
        self.session.lua.remove_interrupt();
        let bytes = match value {
            Value::Buffer(buffer) => buffer.to_vec(),
            Value::String(value) => value.as_bytes().to_vec(),
            _ => {
                return Err(error(
                    "ASTRA_EMU_DECODER_OUTPUT_TYPE",
                    "decoder callback must return a buffer",
                ))
            }
        };
        if bytes.len() > self.session.limits.output_bytes {
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

fn install_intrinsics(lua: &Lua, vfs: &Table) -> Result<(), LegacyProviderError> {
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
                let mut cipher = Rc4::new_from_slice(&key.to_vec())
                    .map_err(|_| mlua::Error::runtime("ASTRA_EMU_DECODER_RC4_KEY"))?;
                let mut discarded = vec![0; skip as usize];
                cipher.apply_keystream(&mut discarded);
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
        lua.create_function(|lua, bytes: mlua::Buffer| {
            let source = bytes.to_vec();
            let mut out = Vec::new();
            ZlibDecoder::new(source.as_slice())
                .read_to_end(&mut out)
                .map_err(mlua::Error::external)?;
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
        "cp932",
        lua.create_function(|lua, text: String| {
            let (bytes, _, malformed) = SHIFT_JIS.encode(&text);
            if malformed {
                return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_CP932"));
            }
            lua.create_buffer(bytes)
        })
        .map_err(lua_api_error)?,
    )
    .map_err(lua_api_error)?;
    vfs.set(
        "mov_decode",
        lua.create_function(
            |lua,
             (bytes, video_key, entry_key, version, chunk_offset, total_size): (
                mlua::Buffer,
                mlua::Buffer,
                mlua::Buffer,
                u8,
                u64,
                u64,
            )| {
                let mut output = bytes.to_vec();
                let video = video_key.to_vec();
                let entry = entry_key.to_vec();
                if video.len() != 256 || entry.is_empty() {
                    return Err(mlua::Error::runtime("ASTRA_EMU_DECODER_VIDEO_KEY"));
                }
                if version == 0 {
                    let mut table = [0u8; 256];
                    for (index, value) in video.iter().enumerate() {
                        table[*value as usize] = index as u8;
                    }
                    for byte in &mut output {
                        *byte = table[*byte as usize];
                    }
                    return lua.create_buffer(output);
                }
                let key = (0..256)
                    .map(|index| video[index] ^ entry[index % entry.len()])
                    .collect::<Vec<_>>();
                let mut cipher = Rc4::new_from_slice(&key)
                    .map_err(|_| mlua::Error::runtime("ASTRA_EMU_DECODER_VIDEO_RC4"))?;
                let block_len = usize::try_from(total_size.min(0x10000))
                    .map_err(|_| mlua::Error::runtime("ASTRA_EMU_DECODER_VIDEO_SIZE"))?;
                if block_len == 0 {
                    return lua.create_buffer(output);
                }
                let mut block = vec![0; block_len];
                cipher.apply_keystream(&mut block);
                for (index, byte) in output.iter_mut().enumerate() {
                    *byte ^= block[(chunk_offset as usize + index) % block.len()];
                }
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
fn error(code: &'static str, message: impl Into<String>) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}
fn lua_api_error(_: mlua::Error) -> LegacyProviderError {
    error(
        "ASTRA_EMU_DECODER_API",
        "trusted decoder API initialization failed",
    )
}
fn lua_call_error(lua_error: mlua::Error) -> LegacyProviderError {
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
    #[test]
    fn buffer_callback_registers_and_runs() {
        let source = r#"astra.vfs.register_decoder({ id='copy', capabilities={'astra.vfs.decode.v1'}, decode_index=function(bytes, descriptor) return bytes end, decode_entries=function(bytes, entries) return bytes end })"#;
        let session = Arc::new(
            TrustedDecoderSession::load(
                source,
                &BTreeSet::from([DECODER_CAPABILITY.into()]),
                DecoderLimits::default(),
            )
            .unwrap(),
        );
        assert_eq!(
            session
                .decoder("copy")
                .unwrap()
                .decode_index("scr", 1, b"abc")
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

    #[test]
    fn entry_callback_receives_bounded_chunks_with_global_offsets() {
        let source = r#"
            astra.vfs.register_decoder({
                id='chunk-offset',
                capabilities={'astra.vfs.decode.v1'},
                decode_index=function(bytes, descriptor) return bytes end,
                decode_entries=function(bytes, entries)
                    local descriptor = entries[1]
                    local output = buffer.create(buffer.len(bytes))
                    buffer.fill(
                        output,
                        0,
                        descriptor.chunk_offset / 4194304,
                        buffer.len(bytes)
                    )
                    return output
                end
            })
        "#;
        let session = Arc::new(
            TrustedDecoderSession::load(
                source,
                &BTreeSet::from([DECODER_CAPABILITY.into()]),
                DecoderLimits::default(),
            )
            .unwrap(),
        );
        let decoder = session.decoder("chunk-offset").unwrap();
        let encrypted = vec![0x5a; DECODER_CHUNK_BYTES + 3];
        let descriptor = PazEntryDescriptor {
            archive_role: "scr".into(),
            entry_id: "entry-1".into(),
            name: "fixture.sc".into(),
            offset: 0,
            unpacked_size: encrypted.len() as u64,
            stored_size: encrypted.len() as u64,
            aligned_size: encrypted.len() as u64,
            packed: false,
            video_key: None,
        };
        let decoded = decoder.decode_entry(2, &descriptor, &encrypted).unwrap();
        assert_eq!(
            &decoded[..DECODER_CHUNK_BYTES],
            vec![0; DECODER_CHUNK_BYTES]
        );
        assert_eq!(&decoded[DECODER_CHUNK_BYTES..], &[1, 1, 1]);
    }
}
