//! Data-only native primitives. Private payloads never enter diagnostics.
//! Native blocks/output are capped at 64MiB; TOML additionally checks the
//! mature parser's derived token/event/value scratch-allocation upper bounds.
use crate::{
    buffer::{self, bounded, BoundedWriter, MAX_NATIVE_BYTES},
    error::check_cancelled,
    PatchError, PatchErrorKind,
};
use encoding_rs::{DecoderResult, EncoderResult, Encoding, GBK, SHIFT_JIS};
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits};
use md5::Md5;
use mlua::{Buffer, Lua, LuaSerdeExt, Table, Value};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom, Write},
    sync::{atomic::AtomicBool, Arc},
};

const CHUNK: usize = 1024 * 1024;
// This is the representation used by toml::Value's serde datetime adapter.
const DATETIME_KEY: &str = "$__toml_private_datetime";

pub(crate) fn install(lua: &Lua, ctx: &Table, cancelled: Arc<AtomicBool>) -> mlua::Result<()> {
    let hash = lua.create_table()?;
    for (name, algorithm) in [
        ("sha256", Hash::Sha256),
        ("md5", Hash::Md5),
        ("crc32", Hash::Crc32),
        ("crc16_ibm", Hash::Crc16),
    ] {
        let cancelled = cancelled.clone();
        hash.set(
            name,
            lua.create_function(move |lua, bytes: Buffer| {
                let result =
                    buffer::input(lua, bytes, |bytes| hash_bytes(bytes, algorithm, &cancelled))?;
                match result {
                    HashValue::Hex(text) => lua.to_value(&text),
                    HashValue::Number(number) => lua.to_value(&number),
                }
            })?,
        )?;
    }
    ctx.set("hash", hash)?;

    let encoding = lua.create_table()?;
    let flag = cancelled.clone();
    encoding.set(
        "decode",
        lua.create_function(move |lua, (bytes, name): (Buffer, mlua::String)| {
            let encoding = TextEncoding::parse(&name.as_bytes()).map_err(mlua::Error::external)?;
            let text = buffer::input(lua, bytes, |bytes| decode_text(bytes, encoding, &flag))?;
            lua.create_string(&text)
        })?,
    )?;
    let flag = cancelled.clone();
    encoding.set(
        "encode",
        lua.create_function(move |lua, (text, name): (mlua::String, mlua::String)| {
            let encoding = TextEncoding::parse(&name.as_bytes()).map_err(mlua::Error::external)?;
            let bytes = text.as_bytes();
            bounded(bytes.len()).map_err(mlua::Error::external)?;
            let text = strict_utf8(&bytes, &flag).map_err(mlua::Error::external)?;
            if matches!(encoding, TextEncoding::Utf8) {
                return buffer::output(lua, &bytes);
            }
            let encoded = encode_text(text, encoding, &flag).map_err(mlua::Error::external)?;
            buffer::output(lua, &encoded)
        })?,
    )?;
    ctx.set("encoding", encoding)?;

    let json = lua.create_table()?;
    let flag = cancelled.clone();
    json.set(
        "decode",
        lua.create_function(move |lua, text: mlua::String| {
            let bytes = text.as_bytes();
            bounded(bytes.len()).map_err(mlua::Error::external)?;
            check_cancelled(&flag).map_err(mlua::Error::external)?;
            // Stream directly into VM-owned data, not an unbounded native Value tree.
            let reader = BufReader::new(CheckedReader::new(&bytes, &flag));
            let mut parser = serde_json::Deserializer::from_reader(reader);
            let value = LuaData {
                lua,
                cancelled: &flag,
            }
            .deserialize(&mut parser)
            .and_then(|value| parser.end().map(|()| value))
            .map_err(|cause| mlua::Error::external(json_error(cause, &flag)))?;
            check_cancelled(&flag).map_err(mlua::Error::external)?;
            Ok(value)
        })?,
    )?;
    let flag = cancelled.clone();
    json.set(
        "encode",
        lua.create_function(move |lua, value: Value| {
            check_cancelled(&flag).map_err(mlua::Error::external)?;
            let mut writer = CheckedWriter::new(&flag);
            if let Err(cause) = serde_json::to_writer(&mut writer, &value) {
                return Err(mlua::Error::external(
                    writer.error("JSON encoding failed", json_diagnostic(&cause)),
                ));
            }
            check_cancelled(&flag).map_err(mlua::Error::external)?;
            lua.create_string(writer.finish())
        })?,
    )?;
    ctx.set("json", json)?;

    let toml = lua.create_table()?;
    let flag = cancelled.clone();
    toml.set(
        "decode",
        lua.create_function(move |lua, text: mlua::String| {
            let bytes = text.as_bytes();
            bounded(bytes.len()).map_err(mlua::Error::external)?;
            let text = strict_utf8(&bytes, &flag).map_err(mlua::Error::external)?;
            toml_budget(text, &flag).map_err(mlua::Error::external)?;
            let parser = toml::de::Deserializer::parse(text)
                .map_err(|cause| mlua::Error::external(toml_error(cause, &flag)))?;
            let value = LuaData {
                lua,
                cancelled: &flag,
            }
            .deserialize(parser)
            .map_err(|cause| mlua::Error::external(toml_error(cause, &flag)))?;
            check_cancelled(&flag).map_err(mlua::Error::external)?;
            Ok(value)
        })?,
    )?;
    let flag = cancelled.clone();
    toml.set(
        "encode",
        lua.create_function(move |lua, value: Value| {
            check_cancelled(&flag).map_err(mlua::Error::external)?;
            let mut writer = CheckedWriter::new(&flag);
            // Use mlua's data-only serde bridge and emit to a bounded writer. The
            // toml document Serializer instead owns an unbounded intermediate tree.
            let parser = mlua::serde::Deserializer::new(value);
            if (TomlData {
                writer: &mut writer,
                root: true,
            })
            .deserialize(parser)
            .is_err()
            {
                return Err(mlua::Error::external(
                    writer.error("TOML encoding failed", Diagnostic::stage("TOML data")),
                ));
            }
            check_cancelled(&flag).map_err(mlua::Error::external)?;
            lua.create_string(writer.finish())
        })?,
    )?;
    ctx.set("toml", toml)?;

    let images = lua.create_table()?;
    let flag = cancelled.clone();
    images.set(
        "decode",
        lua.create_function(move |lua, bytes: Buffer| {
            let (width, height, rgba) =
                buffer::input(lua, bytes, |bytes| decode_image(bytes, &flag))?;
            let result = lua.create_table()?;
            result.set("width", width)?;
            result.set("height", height)?;
            result.set("rgba", buffer::output(lua, &rgba)?)?;
            Ok(result)
        })?,
    )?;
    images.set(
        "png",
        lua.create_function(move |lua, (width, height, bytes): (Value, Value, Buffer)| {
            let width = dimension(width)?;
            let height = dimension(height)?;
            let length = rgba_length(width, height).map_err(mlua::Error::external)?;
            let png = buffer::input(lua, bytes, |bytes| {
                if bytes.len() != length {
                    return Err(bounds("RGBA pixel count does not match dimensions"));
                }
                check_cancelled(&cancelled)?;
                let mut writer = CheckedWriter::new(&cancelled);
                image::codecs::png::PngEncoder::new(&mut writer)
                    .write_image(bytes, width, height, image::ExtendedColorType::Rgba8)
                    .map_err(|_| {
                        writer.error("PNG encoding failed", Diagnostic::stage("PNG encoding"))
                    })?;
                check_cancelled(&cancelled)?;
                Ok(writer.finish())
            })?;
            buffer::output(lua, &png)
        })?,
    )?;
    ctx.set("image", images)
}

fn bounds(message: &'static str) -> PatchError {
    PatchError::new(PatchErrorKind::Bounds, message)
}
fn codec(message: &'static str) -> PatchError {
    PatchError::new(PatchErrorKind::Codec, message)
}

fn text_io(cause: io::Error, message: &'static str) -> PatchError {
    let kind = cause
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<PatchError>())
        .map(|cause| cause.kind())
        .unwrap_or_else(|| match cause.kind() {
            io::ErrorKind::FileTooLarge => PatchErrorKind::Bounds,
            io::ErrorKind::OutOfMemory => PatchErrorKind::Memory,
            _ => PatchErrorKind::Codec,
        });
    PatchError::with_source(kind, message, cause)
}

// Only these safe fields survive parser errors; parser Display/Debug can contain
// whole private lines, names, or unescaped string values.
#[derive(Debug)]
struct Diagnostic {
    stage: &'static str,
    line: usize,
    column: usize,
    offset: usize,
}
impl Diagnostic {
    fn stage(stage: &'static str) -> Self {
        Self {
            stage,
            line: 0,
            column: 0,
            offset: 0,
        }
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}, byte {}",
            self.stage, self.line, self.column, self.offset
        )
    }
}
impl std::error::Error for Diagnostic {}
fn json_diagnostic(cause: &serde_json::Error) -> Diagnostic {
    Diagnostic {
        stage: "JSON data",
        line: cause.line(),
        column: cause.column(),
        offset: 0,
    }
}
fn json_error(cause: serde_json::Error, flag: &AtomicBool) -> PatchError {
    if let Err(error) = check_cancelled(flag) {
        return error;
    }
    PatchError::with_source(
        PatchErrorKind::Codec,
        "JSON decoding failed",
        json_diagnostic(&cause),
    )
}
fn toml_error(cause: toml::de::Error, flag: &AtomicBool) -> PatchError {
    if let Err(error) = check_cancelled(flag) {
        return error;
    }
    let diagnostic = Diagnostic {
        stage: "TOML data",
        offset: cause.span().map_or(0, |span| span.start),
        line: 0,
        column: 0,
    };
    PatchError::with_source(PatchErrorKind::Codec, "TOML decoding failed", diagnostic)
}

fn toml_budget(text: &str, cancelled: &AtomicBool) -> Result<(), PatchError> {
    use std::mem::{align_of, size_of};
    use toml_parser::{
        lexer::{Token, TokenKind},
        parser::Event,
    };
    check_cancelled(cancelled)?;
    // toml_parser 1.1.2's into_vec reserves input.len() Token slots before
    // lexing, even for one long literal. Bound that actual scratch allocation.
    toml_allocation(text.len().checked_mul(size_of::<Token>()))?;
    // DeArray grows Vec<Spanned<DeValue>> geometrically. Count potential
    // values, not text bytes: long literals do not imply large value arrays.
    // The event stream has at most two events per token (including recovery).
    // Bounding its geometric capacity also covers its initial token-count cap.
    // This preflight only counts mature lexer tokens; it does not parse TOML.
    let value_slot = (size_of::<toml::de::DeValue<'_>>() + 2 * size_of::<usize>())
        .next_multiple_of(align_of::<toml::de::DeValue<'_>>());
    let mut tokens = 4_usize;
    let mut values = 4_usize;
    for token in toml_parser::Source::new(text).lex() {
        check_cancelled(cancelled)?;
        tokens += 1; // Input is bounded; neither counter can overflow usize.
        let event_capacity = tokens
            .checked_next_power_of_two()
            .and_then(|n| n.checked_mul(2));
        toml_allocation(event_capacity.and_then(|n| n.checked_mul(size_of::<Event>())))?;
        if matches!(
            token.kind(),
            TokenKind::Atom
                | TokenKind::LiteralString
                | TokenKind::BasicString
                | TokenKind::MlLiteralString
                | TokenKind::MlBasicString
                | TokenKind::LeftSquareBracket
                | TokenKind::LeftCurlyBracket
        ) {
            values += 1;
            toml_allocation(
                values
                    .checked_next_power_of_two()
                    .and_then(|n| n.checked_mul(value_slot)),
            )?;
        }
    }
    check_cancelled(cancelled)
}

fn toml_allocation(bytes: Option<usize>) -> Result<(), PatchError> {
    match bytes {
        Some(bytes) if bytes <= MAX_NATIVE_BYTES => Ok(()),
        Some(bytes) => Err(PatchError::new(PatchErrorKind::Memory,
            format!("TOML parser allocation upper bound is {bytes} bytes; limit is {MAX_NATIVE_BYTES} bytes"))),
        None => Err(PatchError::new(PatchErrorKind::Memory, "TOML parser allocation upper bound overflows")),
    }
}

struct CheckedWriter<'a> {
    inner: BoundedWriter,
    cancelled: &'a AtomicBool,
    failure: Option<PatchErrorKind>,
}
impl<'a> CheckedWriter<'a> {
    fn new(cancelled: &'a AtomicBool) -> Self {
        Self {
            inner: BoundedWriter::new(),
            cancelled,
            failure: None,
        }
    }
    fn finish(self) -> Vec<u8> {
        self.inner.finish()
    }
    fn error(&self, message: &'static str, cause: Diagnostic) -> PatchError {
        let kind = if check_cancelled(self.cancelled).is_err() {
            PatchErrorKind::Cancelled
        } else {
            self.failure.unwrap_or(PatchErrorKind::Codec)
        };
        PatchError::with_source(kind, message, cause)
    }
}
impl Write for CheckedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        check_cancelled(self.cancelled).map_err(io::Error::other)?;
        let result = self.inner.write(&bytes[..bytes.len().min(CHUNK)]);
        if let Err(cause) = &result {
            self.failure = Some(match cause.kind() {
                io::ErrorKind::FileTooLarge => PatchErrorKind::Bounds,
                io::ErrorKind::OutOfMemory => PatchErrorKind::Memory,
                _ => PatchErrorKind::Codec,
            });
        }
        result
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct CheckedReader<'a> {
    inner: Cursor<&'a [u8]>,
    cancelled: &'a AtomicBool,
}
impl<'a> CheckedReader<'a> {
    fn new(bytes: &'a [u8], cancelled: &'a AtomicBool) -> Self {
        Self {
            inner: Cursor::new(bytes),
            cancelled,
        }
    }
}
impl Read for CheckedReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        check_cancelled(self.cancelled).map_err(io::Error::other)?;
        let length = bytes.len().min(CHUNK);
        self.inner.read(&mut bytes[..length])
    }
}
impl Seek for CheckedReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        check_cancelled(self.cancelled).map_err(io::Error::other)?;
        self.inner.seek(position)
    }
}

#[derive(Clone, Copy)]
enum Hash {
    Sha256,
    Md5,
    Crc32,
    Crc16,
}
enum HashValue {
    Hex(String),
    Number(u32),
}
const CRC16_TABLE: [u16; 256] = {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u16;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ (0xA001 & (0_u16.wrapping_sub(crc & 1)));
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
};
fn hash_bytes(
    bytes: &[u8],
    algorithm: Hash,
    cancelled: &AtomicBool,
) -> Result<HashValue, PatchError> {
    bounded(bytes.len())?;
    check_cancelled(cancelled)?;
    match algorithm {
        Hash::Sha256 => {
            let mut hash = Sha256::new();
            for chunk in bytes.chunks(CHUNK) {
                check_cancelled(cancelled)?;
                hash.update(chunk);
            }
            check_cancelled(cancelled)?;
            Ok(HashValue::Hex(hex(&hash.finalize())))
        }
        Hash::Md5 => {
            let mut hash = Md5::new();
            for chunk in bytes.chunks(CHUNK) {
                check_cancelled(cancelled)?;
                hash.update(chunk);
            }
            check_cancelled(cancelled)?;
            Ok(HashValue::Hex(hex(&hash.finalize())))
        }
        Hash::Crc32 => {
            let mut hash = crc32fast::Hasher::new();
            for chunk in bytes.chunks(CHUNK) {
                check_cancelled(cancelled)?;
                hash.update(chunk);
            }
            check_cancelled(cancelled)?;
            Ok(HashValue::Number(hash.finalize()))
        }
        Hash::Crc16 => {
            let mut crc = 0_u16;
            for chunk in bytes.chunks(CHUNK) {
                check_cancelled(cancelled)?;
                for byte in chunk {
                    crc = (crc >> 8) ^ CRC16_TABLE[((crc ^ u16::from(*byte)) & 255) as usize];
                }
            }
            check_cancelled(cancelled)?;
            Ok(HashValue::Number(u32::from(crc)))
        }
    }
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 15) as usize] as char);
    }
    output
}

#[derive(Clone, Copy)]
enum TextEncoding {
    Utf8,
    Utf16Le,
    Legacy(&'static Encoding),
}
impl TextEncoding {
    fn parse(name: &[u8]) -> Result<Self, PatchError> {
        match name {
            b"utf8" => Ok(Self::Utf8),
            b"utf16le" => Ok(Self::Utf16Le),
            b"cp932" => Ok(Self::Legacy(SHIFT_JIS)),
            b"gbk" => Ok(Self::Legacy(GBK)),
            _ => Err(codec("unsupported encoding name")),
        }
    }
}
fn strict_utf8<'a>(bytes: &'a [u8], cancelled: &AtomicBool) -> Result<&'a str, PatchError> {
    let mut position = 0;
    while position < bytes.len() {
        check_cancelled(cancelled)?;
        let end = bytes.len().min(position.saturating_add(CHUNK));
        match std::str::from_utf8(&bytes[position..end]) {
            Ok(_) => position = end,
            Err(cause) if cause.error_len().is_none() && end < bytes.len() => {
                position += cause.valid_up_to()
            }
            Err(cause) => {
                return Err(PatchError::with_source(
                    PatchErrorKind::Codec,
                    "invalid UTF-8 data",
                    cause,
                ))
            }
        }
    }
    check_cancelled(cancelled)?;
    // SAFETY: every byte belongs to a validated UTF-8 segment. An incomplete
    // scalar at a chunk end is revalidated with the following input segment.
    Ok(unsafe { std::str::from_utf8_unchecked(bytes) })
}
fn decode_text(
    bytes: &[u8],
    encoding: TextEncoding,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, PatchError> {
    bounded(bytes.len())?;
    check_cancelled(cancelled)?;
    let mut writer = CheckedWriter::new(cancelled);
    match encoding {
        TextEncoding::Utf8 => {
            strict_utf8(bytes, cancelled)?;
            writer
                .write_all(bytes)
                .map_err(|cause| text_io(cause, "text decoding failed"))?;
        }
        TextEncoding::Utf16Le => {
            if !bytes.len().is_multiple_of(2) {
                return Err(codec("UTF-16LE data has an odd byte count"));
            }
            let units = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair));
            let mut output = [0; 8192];
            let mut filled = 0;
            for scalar in char::decode_utf16(units) {
                if filled > output.len() - 4 {
                    writer
                        .write_all(&output[..filled])
                        .map_err(|cause| text_io(cause, "text decoding failed"))?;
                    filled = 0;
                }
                let scalar = scalar.map_err(|_| {
                    PatchError::with_source(
                        PatchErrorKind::Codec,
                        "invalid UTF-16LE data",
                        Diagnostic::stage("UTF-16LE surrogate"),
                    )
                })?;
                filled += scalar.encode_utf8(&mut output[filled..]).len();
            }
            writer
                .write_all(&output[..filled])
                .map_err(|cause| text_io(cause, "text decoding failed"))?;
        }
        TextEncoding::Legacy(encoding) => {
            let mut decoder = encoding.new_decoder_without_bom_handling();
            let mut position: usize = 0;
            let mut output = [0; 8192];
            loop {
                check_cancelled(cancelled)?;
                let end = bytes.len().min(position.saturating_add(CHUNK));
                let (result, read, written) = decoder.decode_to_utf8_without_replacement(
                    &bytes[position..end],
                    &mut output,
                    end == bytes.len(),
                );
                position += read;
                writer
                    .write_all(&output[..written])
                    .map_err(|cause| text_io(cause, "text decoding failed"))?;
                match result {
                    DecoderResult::Malformed(_, _) => return Err(codec("invalid encoded text")),
                    DecoderResult::InputEmpty if position == bytes.len() => break,
                    _ => {}
                }
            }
        }
    }
    check_cancelled(cancelled)?;
    Ok(writer.finish())
}
fn encode_text(
    text: &str,
    encoding: TextEncoding,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, PatchError> {
    bounded(text.len())?;
    check_cancelled(cancelled)?;
    let mut writer = CheckedWriter::new(cancelled);
    match encoding {
        TextEncoding::Utf8 => writer.write_all(text.as_bytes()),
        TextEncoding::Utf16Le => {
            let mut output = [0; 8192];
            let mut filled = 0;
            for unit in text.encode_utf16() {
                if filled == output.len() {
                    writer
                        .write_all(&output)
                        .map_err(|cause| text_io(cause, "text encoding failed"))?;
                    filled = 0;
                }
                output[filled..filled + 2].copy_from_slice(&unit.to_le_bytes());
                filled += 2;
            }
            writer.write_all(&output[..filled])
        }
        TextEncoding::Legacy(encoding) => {
            let mut encoder = encoding.new_encoder();
            let mut position: usize = 0;
            let mut output = [0; 8192];
            loop {
                check_cancelled(cancelled)?;
                let mut end = text.len().min(position.saturating_add(CHUNK));
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                let (result, read, written) = encoder.encode_from_utf8_without_replacement(
                    &text[position..end],
                    &mut output,
                    end == text.len(),
                );
                position += read;
                writer
                    .write_all(&output[..written])
                    .map_err(|cause| text_io(cause, "text encoding failed"))?;
                match result {
                    EncoderResult::Unmappable(_) => {
                        return Err(codec("text is not representable in the requested encoding"))
                    }
                    EncoderResult::InputEmpty if position == text.len() => break,
                    _ => {}
                }
            }
            Ok(())
        }
    }
    .map_err(|cause| text_io(cause, "text encoding failed"))?;
    check_cancelled(cancelled)?;
    Ok(writer.finish())
}

// Deserialize through the mature format parsers directly into VM-managed data.
// LuaSerdeExt supplies primitive/null semantics and the standard array marker.
struct LuaData<'a> {
    lua: &'a Lua,
    cancelled: &'a AtomicBool,
}
impl<'de> DeserializeSeed<'de> for LuaData<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, parser: D) -> Result<Value, D::Error> {
        check_cancelled(self.cancelled).map_err(|_| de::Error::custom("conversion cancelled"))?;
        parser.deserialize_any(self)
    }
}
impl LuaData<'_> {
    fn primitive<E: de::Error, T: serde::Serialize + ?Sized>(&self, value: &T) -> Result<Value, E> {
        self.lua
            .to_value(value)
            .map_err(|_| E::custom("Lua data allocation failed"))
    }
}
impl<'de> Visitor<'de> for LuaData<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("data")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(self.lua.null())
    }
    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(self.lua.null())
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        self.primitive(&value)
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        self.primitive(&value)
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        self.primitive(&value)
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        self.primitive(&value)
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        bounded(value.len()).map_err(|_| E::custom("native text exceeds 64MiB"))?;
        self.primitive(value)
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        self.visit_str(&value)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<Value, A::Error> {
        let table = self
            .lua
            .create_table()
            .map_err(|_| de::Error::custom("Lua data allocation failed"))?;
        table.set_metatable(Some(self.lua.array_metatable()));
        let mut index = 1_usize;
        while let Some(value) = values.next_element_seed(LuaData {
            lua: self.lua,
            cancelled: self.cancelled,
        })? {
            table
                .raw_set(index, value)
                .map_err(|_| de::Error::custom("Lua data allocation failed"))?;
            index = index
                .checked_add(1)
                .ok_or_else(|| de::Error::custom("array length overflow"))?;
        }
        Ok(Value::Table(table))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<Value, A::Error> {
        let table = self
            .lua
            .create_table()
            .map_err(|_| de::Error::custom("Lua data allocation failed"))?;
        while let Some(key) = values.next_key_seed(TextKey)? {
            let value = values.next_value_seed(LuaData {
                lua: self.lua,
                cancelled: self.cancelled,
            })?;
            table
                .raw_set(key, value)
                .map_err(|_| de::Error::custom("Lua data allocation failed"))?;
        }
        Ok(Value::Table(table))
    }
}

// A fallibly allocated key; numeric/object keys remain errors, as in toml::Value.
struct TextKey;
impl<'de> DeserializeSeed<'de> for TextKey {
    type Value = String;
    fn deserialize<D: de::Deserializer<'de>>(self, parser: D) -> Result<String, D::Error> {
        parser.deserialize_str(self)
    }
}
impl<'de> Visitor<'de> for TextKey {
    type Value = String;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UTF-8 text")
    }
    fn visit_str<E: de::Error>(self, text: &str) -> Result<String, E> {
        bounded(text.len()).map_err(|_| E::custom("native text exceeds 64MiB"))?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(text.len())
            .map_err(|_| E::custom("native text allocation failed"))?;
        owned.push_str(text);
        Ok(owned)
    }
    fn visit_string<E: de::Error>(self, text: String) -> Result<String, E> {
        bounded(text.capacity()).map_err(|_| E::custom("native text exceeds 64MiB"))?;
        Ok(text)
    }
}

// Only TOML's data representation is emitted here; parsing stays in toml.
// Arrays and inline tables avoid a second native document/tree allocation.
struct TomlData<'a, 'b> {
    writer: &'a mut CheckedWriter<'b>,
    root: bool,
}
impl<'de> DeserializeSeed<'de> for TomlData<'_, '_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, parser: D) -> Result<(), D::Error> {
        check_cancelled(self.writer.cancelled)
            .map_err(|_| de::Error::custom("conversion cancelled"))?;
        parser.deserialize_any(self)
    }
}
impl TomlData<'_, '_> {
    fn scalar<E: de::Error>(&self) -> Result<(), E> {
        if self.root {
            Err(E::custom("TOML document must be a table"))
        } else {
            Ok(())
        }
    }
    fn write<E: de::Error>(&mut self, bytes: &[u8]) -> Result<(), E> {
        self.writer
            .write_all(bytes)
            .map_err(|_| E::custom("TOML output failed"))
    }
    fn quoted<E: de::Error>(&mut self, text: &str) -> Result<(), E> {
        self.write(b"\"")?;
        let mut start = 0;
        for (index, byte) in text.bytes().enumerate() {
            if index % CHUNK == 0 {
                check_cancelled(self.writer.cancelled)
                    .map_err(|_| E::custom("conversion cancelled"))?;
            }
            if byte == b'"' || byte == b'\\' || byte < 0x20 || byte == 0x7f {
                self.write(&text.as_bytes()[start..index])?;
                match byte {
                    b'"' => self.write(b"\\\"")?,
                    b'\\' => self.write(b"\\\\")?,
                    _ => {
                        const HEX: &[u8; 16] = b"0123456789abcdef";
                        self.write(&[
                            b'\\',
                            b'u',
                            b'0',
                            b'0',
                            HEX[(byte >> 4) as usize],
                            HEX[(byte & 15) as usize],
                        ])?;
                    }
                }
                start = index + 1;
            }
        }
        self.write(&text.as_bytes()[start..])?;
        self.write(b"\"")
    }
}
impl<'de> Visitor<'de> for TomlData<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TOML data")
    }
    fn visit_bool<E: de::Error>(mut self, value: bool) -> Result<(), E> {
        self.scalar()?;
        self.write(if value { b"true" } else { b"false" })
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<(), E> {
        self.scalar()?;
        write!(self.writer, "{value}").map_err(|_| E::custom("TOML output failed"))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<(), E> {
        let value = i64::try_from(value).map_err(|_| E::custom("TOML integer overflow"))?;
        self.visit_i64(value)
    }
    fn visit_f64<E: de::Error>(mut self, value: f64) -> Result<(), E> {
        self.scalar()?;
        if value.is_nan() {
            return self.write(if value.is_sign_negative() {
                b"-nan"
            } else {
                b"nan"
            });
        }
        if value.is_infinite() {
            return self.write(if value.is_sign_negative() {
                b"-inf"
            } else {
                b"inf"
            });
        }
        write!(self.writer, "{value}").map_err(|_| E::custom("TOML output failed"))?;
        if value.fract() == 0.0 {
            self.write(b".0")?;
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(mut self, value: &str) -> Result<(), E> {
        self.scalar()?;
        self.quoted(value)
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<(), E> {
        self.visit_str(&value)
    }
    fn visit_seq<A: SeqAccess<'de>>(mut self, mut values: A) -> Result<(), A::Error> {
        self.scalar()?;
        self.write(b"[")?;
        let mut first = true;
        loop {
            // Put the separator in the element seed, not before probing EOF.
            let seed = TomlElement {
                writer: self.writer,
                separator: !first,
            };
            if values.next_element_seed(seed)?.is_none() {
                break;
            }
            first = false;
        }
        self.write(b"]")
    }
    fn visit_map<A: MapAccess<'de>>(mut self, mut values: A) -> Result<(), A::Error> {
        let first = values.next_key_seed(TextKey)?;
        if first.as_deref() == Some(DATETIME_KEY) {
            self.scalar()?;
            let text = values.next_value_seed(TextKey)?;
            let date: toml::value::Datetime = text
                .parse()
                .map_err(|_| de::Error::custom("invalid TOML datetime"))?;
            if values.next_key_seed(TextKey)?.is_some() {
                return Err(de::Error::custom("invalid TOML datetime map"));
            }
            return write!(self.writer, "{date}")
                .map_err(|_| de::Error::custom("TOML output failed"));
        }
        if !self.root {
            self.write(b"{")?;
        }
        let mut key = first;
        let mut first = true;
        while let Some(name) = key {
            if !first && !self.root {
                self.write(b", ")?;
            }
            self.quoted(&name)?;
            self.write(b" = ")?;
            values.next_value_seed(TomlData {
                writer: self.writer,
                root: false,
            })?;
            if self.root {
                self.write(b"\n")?;
            }
            first = false;
            key = values.next_key_seed(TextKey)?;
        }
        if !self.root {
            self.write(b"}")?;
        }
        Ok(())
    }
}
struct TomlElement<'a, 'b> {
    writer: &'a mut CheckedWriter<'b>,
    separator: bool,
}
impl<'de> DeserializeSeed<'de> for TomlElement<'_, '_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, parser: D) -> Result<(), D::Error> {
        if self.separator {
            self.writer
                .write_all(b", ")
                .map_err(|_| de::Error::custom("TOML output failed"))?;
        }
        TomlData {
            writer: self.writer,
            root: false,
        }
        .deserialize(parser)
    }
}

fn dimension(value: Value) -> mlua::Result<u32> {
    let dimension = buffer::integer(value)?;
    u32::try_from(dimension)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            mlua::Error::external(bounds("image dimensions must be positive u32 integers"))
        })
}
fn rgba_length(width: u32, height: u32) -> Result<usize, PatchError> {
    if width == 0 || height == 0 {
        return Err(bounds("image dimensions must be positive"));
    }
    let length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|length| length.checked_mul(4))
        .ok_or_else(|| bounds("image dimensions overflow"))?;
    bounded(length)?;
    Ok(length)
}
fn decode_image(bytes: &[u8], cancelled: &AtomicBool) -> Result<(u32, u32, Vec<u8>), PatchError> {
    bounded(bytes.len())?;
    check_cancelled(cancelled)?;
    let format = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ImageFormat::Png
    } else if bytes.starts_with(b"BM") {
        ImageFormat::Bmp
    } else {
        return Err(PatchError::new(
            PatchErrorKind::UnsupportedInput,
            "only PNG and BMP images are supported",
        ));
    };
    let failure = |_| {
        if let Err(error) = check_cancelled(cancelled) {
            error
        } else {
            PatchError::with_source(
                PatchErrorKind::Codec,
                "image decoding failed",
                Diagnostic::stage("image decoding"),
            )
        }
    };
    let reader = BufReader::new(CheckedReader::new(bytes, cancelled));
    let mut reader = ImageReader::with_format(reader, format);
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_NATIVE_BYTES as u64);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(failure)?;
    let (width, height) = decoder.dimensions();
    let length = rgba_length(width, height)?;
    let decoded_length = usize::try_from(decoder.total_bytes())
        .map_err(|_| bounds("decoded image length overflows"))?;
    bounded(decoded_length)?;
    // image's conversion is needed for palette, RGB, grayscale and 16-bit PNG.
    // The decoder and converted RGBA block are each bounded before allocation.
    let decoded = DynamicImage::from_decoder(decoder).map_err(failure)?;
    check_cancelled(cancelled)?;
    let rgba = decoded.into_rgba8().into_raw();
    if rgba.len() != length {
        return Err(codec("decoded image pixel count is inconsistent"));
    }
    check_cancelled(cancelled)?;
    Ok((width, height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{error::Error, sync::atomic::Ordering};

    fn runtime() -> (Lua, Arc<AtomicBool>) {
        let lua = Lua::new();
        lua.set_memory_limit(256 * 1024 * 1024).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let ctx = lua.create_table().unwrap();
        install(&lua, &ctx, cancelled.clone()).unwrap();
        lua.globals().set("ctx", ctx).unwrap();
        (lua, cancelled)
    }

    #[test]
    fn lua_hash_public_vectors_and_chunk_boundary() {
        let (lua, _) = runtime();
        lua.load(r#"
            local abc = buffer.fromstring("abc")
            assert(ctx.hash.sha256(abc) == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
            assert(ctx.hash.md5(abc) == "900150983cd24fb0d6963f7d28e17f72")
            local digits = buffer.fromstring("123456789")
            assert(ctx.hash.crc32(digits) == 0xCBF43926)
            assert(ctx.hash.crc16_ibm(digits) == 0xBB3D)
            local empty = buffer.create(0)
            assert(ctx.hash.sha256(empty) == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
            assert(ctx.hash.md5(empty) == "d41d8cd98f00b204e9800998ecf8427e")
            assert(ctx.hash.crc32(empty) == 0 and ctx.hash.crc16_ibm(empty) == 0)
        "#).exec().unwrap();
        let bytes = [vec![b'a'; CHUNK], b"bc".to_vec()].concat();
        lua.globals()
            .set("large", lua.create_buffer(&bytes).unwrap())
            .unwrap();
        // One-shot reference checks the streamed final partial chunk.
        let expected_sha = hex(&Sha256::digest(&bytes));
        let expected_md5 = hex(&Md5::digest(&bytes));
        let actual: (String, String, u32, u32) = lua.load("return ctx.hash.sha256(large), ctx.hash.md5(large), ctx.hash.crc32(large), ctx.hash.crc16_ibm(large)").eval().unwrap();
        assert_eq!(actual.0, expected_sha);
        assert_eq!(actual.1, expected_md5);
        assert_eq!(actual.2, crc32fast::hash(&bytes));
        let mut crc = 0_u16;
        for byte in bytes {
            crc ^= u16::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xA001
                } else {
                    crc >> 1
                };
            }
        }
        assert_eq!(actual.3, u32::from(crc));
    }

    #[test]
    fn lua_strict_encoding_public_vectors() {
        let (lua, _) = runtime();
        lua.load(
            r#"
            local function bytes(...) return buffer.fromstring(string.char(...)) end
            local function same(a, b) assert(buffer.tostring(a) == buffer.tostring(b)) end
            assert(ctx.encoding.decode(bytes(0x82, 0xA0), "cp932") == "あ")
            assert(ctx.encoding.decode(bytes(0xD6, 0xD0, 0xCE, 0xC4), "gbk") == "中文")
            assert(ctx.encoding.decode(bytes(0x41, 0, 0x3D, 0xD8, 0, 0xDE), "utf16le") == "A😀")
            same(ctx.encoding.encode("あ", "cp932"), bytes(0x82, 0xA0))
            same(ctx.encoding.encode("中文", "gbk"), bytes(0xD6, 0xD0, 0xCE, 0xC4))
            same(ctx.encoding.encode("A😀", "utf16le"), bytes(0x41, 0, 0x3D, 0xD8, 0, 0xDE))
            for _, name in {"utf8", "utf16le", "cp932", "gbk"} do
                same(ctx.encoding.encode("", name), buffer.create(0))
                assert(ctx.encoding.decode(buffer.create(0), name) == "")
                local text = "A\0"
                assert(ctx.encoding.decode(ctx.encoding.encode(text, name), name) == text)
            end
            local text = "A😀中文"
            assert(ctx.encoding.decode(ctx.encoding.encode(text, "utf8"), "utf8") == text)
            -- UTF-16LE preserves BOM as data rather than switching byte order.
            assert(ctx.encoding.decode(bytes(0xFF, 0xFE, 0x41, 0), "utf16le") == "\u{FEFF}A")
            for _, name in {"UTF8", "utf-8", "shift_jis", "windows-31j", "GBK", ""} do
                assert(not pcall(ctx.encoding.encode, "A", name))
                assert(not pcall(ctx.encoding.decode, bytes(65), name))
            end
            for _, bad in {bytes(0xC0, 0xAF), bytes(0xED, 0xA0, 0x80), bytes(0xE2, 0x82)} do
                assert(not pcall(ctx.encoding.decode, bad, "utf8"))
                assert(not pcall(ctx.encoding.encode, buffer.tostring(bad), "utf8"))
            end
            for _, bad in {bytes(0x41), bytes(0, 0xD8), bytes(0, 0xDC), bytes(0, 0xD8, 65, 0)} do
                assert(not pcall(ctx.encoding.decode, bad, "utf16le"))
            end
            assert(not pcall(ctx.encoding.decode, bytes(0x81), "cp932"))
            assert(not pcall(ctx.encoding.decode, bytes(0x81, 0x7F), "cp932"))
            assert(not pcall(ctx.encoding.decode, bytes(0x81), "gbk"))
            assert(not pcall(ctx.encoding.decode, bytes(0x81, 0x20), "gbk"))
            assert(not pcall(ctx.encoding.encode, "😀", "cp932"))
            assert(not pcall(ctx.encoding.encode, "😀", "gbk"))
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn lua_streaming_legacy_character_boundaries() {
        let (lua, _) = runtime();
        lua.load(r#"
            local prefix = string.rep("x", 1024 * 1024 - 1)
            local cp932 = buffer.fromstring(prefix .. string.char(0x82, 0xA0) .. "z")
            assert(ctx.encoding.decode(cp932, "cp932") == prefix .. "あz")
            assert(buffer.tostring(ctx.encoding.encode(prefix .. "あz", "cp932")) == buffer.tostring(cp932))
            local gbk = buffer.fromstring(prefix .. string.char(0xD6, 0xD0) .. "z")
            assert(ctx.encoding.decode(gbk, "gbk") == prefix .. "中z")
            assert(buffer.tostring(ctx.encoding.encode(prefix .. "中z", "gbk")) == buffer.tostring(gbk))
            local utf16 = ctx.encoding.encode(prefix .. "😀z", "utf16le")
            assert(ctx.encoding.decode(utf16, "utf16le") == prefix .. "😀z")
        "#).exec().unwrap();
    }

    #[test]
    fn lua_json_nested_null_and_array_semantics() {
        let (lua, _) = runtime();
        let encoded: String = lua.load(r#"
            local value = ctx.json.decode('{"items":[{"name":"中文","enabled":true},null,[]],"count":7,"empty":{}}')
            assert(value.items[1].name == "中文" and value.items[1].enabled)
            assert(value.count == 7 and #value.items == 3 and #value.items[3] == 0)
            return ctx.json.encode(value)
        "#).eval().unwrap();
        let expected: serde_json::Value = serde_json::from_str(
            r#"{"items":[{"name":"中文","enabled":true},null,[]],"count":7,"empty":{}}"#,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
            expected
        );
        lua.load(
            r#"
            assert(ctx.json.encode(ctx.json.decode("null")) == "null")
            assert(ctx.json.encode(ctx.json.decode("[]")) == "[]")
            assert(not pcall(ctx.json.decode, "{} trailing"))
            assert(not pcall(ctx.json.encode, function() end))
            local cycle = {}; cycle.self = cycle
            assert(not pcall(ctx.json.encode, cycle))
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn lua_toml_preserves_representable_values_dates_and_nonfinite_numbers() {
        let (lua, _) = runtime();
        let source = r#"
            title = "中文"
            integer = -42
            integral_float = 2.0
            negative_zero = -0.0
            huge_float = 1e200
            tiny_float = 1e-200
            enabled = true
            list = [{ name = "first", values = [1, 2] }, { name = "second", values = [] }]
            "quote\"slash\\" = "line\n\u0000\t\u007F"
            matrix = [[1, 2], [3, 4]]
            empty_table = {}
            offset_date = 1979-05-27T07:32:00.123-07:00
            local_date_time = 1979-05-27T07:32:00
            local_date = 1979-05-27
            local_time = 07:32:00
            [nested]
            "key.with.dot" = { inner = { x = "y" } }
        "#;
        lua.globals().set("manifest", source).unwrap();
        let emitted: String = lua
            .load(
                r#"
            local data = ctx.toml.decode(manifest)
            assert(data.title == "中文" and data.nested["key.with.dot"].inner.x == "y")
            return ctx.toml.encode(data)
        "#,
            )
            .eval()
            .unwrap();
        let mut expected: toml::Value = toml::from_str(source).unwrap();
        let mut actual: toml::Value = toml::from_str(&emitted).unwrap();
        // Luau has one numeric type: 2 and 2.0 are indistinguishable. Check the
        // value, not its unrecoverable TOML literal spelling.
        let integral = actual
            .as_table_mut()
            .unwrap()
            .remove("integral_float")
            .unwrap();
        let numeric = integral
            .as_float()
            .unwrap_or_else(|| integral.as_integer().unwrap() as f64);
        assert_eq!(numeric, 2.0);
        expected
            .as_table_mut()
            .unwrap()
            .remove("integral_float")
            .unwrap();
        assert_eq!(actual, expected);
        assert!(actual["negative_zero"]
            .as_float()
            .unwrap()
            .is_sign_negative());
        let infinities: String = lua
            .load(
                r#"
            return ctx.toml.encode({pos = math.huge, neg = -math.huge, nan = 0/0})
        "#,
            )
            .eval()
            .unwrap();
        let values: toml::Value = toml::from_str(&infinities).unwrap();
        assert_eq!(values["pos"].as_float(), Some(f64::INFINITY));
        assert_eq!(values["neg"].as_float(), Some(f64::NEG_INFINITY));
        assert!(values["nan"].as_float().unwrap().is_nan());
        lua.load(
            r#"
            assert(ctx.toml.encode({}) == "")
            assert(not pcall(ctx.toml.encode, 42))
            assert(not pcall(ctx.toml.encode, function() end))
            local cycle = {}; cycle.self = cycle
            assert(not pcall(ctx.toml.encode, cycle))
            assert(not pcall(ctx.toml.encode, {x = ctx.json.decode("null")}))
        "#,
        )
        .exec()
        .unwrap();
    }

    fn assert_private_error(error: mlua::Error, private: &str) {
        let mut current: Option<&dyn Error> = Some(&error);
        while let Some(cause) = current {
            assert!(!format!("{cause}").contains(private));
            assert!(!format!("{cause:?}").contains(private));
            current = cause.source();
        }
    }

    #[test]
    fn lua_manifest_parser_errors_never_retain_private_content() {
        let (lua, _) = runtime();
        const PRIVATE: &str = "PRIVATE_MANIFEST_SECRET_8ea4";
        for document in [
            format!("{{\"{PRIVATE}\": invalid}}"),
            format!("{{\"{PRIVATE}\": \"unterminated"),
        ] {
            lua.globals().set("document", document).unwrap();
            assert_private_error(
                lua.load("return ctx.json.decode(document)")
                    .eval::<Value>()
                    .unwrap_err(),
                PRIVATE,
            );
        }
        for document in [
            format!("{PRIVATE} = \"unterminated"),
            format!("{PRIVATE} = [invalid]"),
            format!("{PRIVATE} = 1\n{PRIVATE} = 2"),
        ] {
            lua.globals().set("document", document).unwrap();
            assert_private_error(
                lua.load("return ctx.toml.decode(document)")
                    .eval::<Value>()
                    .unwrap_err(),
                PRIVATE,
            );
        }
        let diagnostic = json_error(
            serde_json::from_str::<serde_json::Value>("{\n bad").unwrap_err(),
            &AtomicBool::new(false),
        );
        let position = diagnostic.source().unwrap().to_string();
        assert!(position.contains("line 2"));
        assert!(!position.contains("bad"));
    }

    // Hand-specified 2x1, bottom-up, 24-bit BMP: red then green, row padded.
    const BMP: &[u8] = &[
        0x42, 0x4D, 62, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 1,
        0, 24, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        255, 0, 255, 0, 0, 0,
    ];

    fn png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
        png.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        png.extend_from_slice(kind);
        png.extend_from_slice(payload);
        let mut crc = u32::MAX;
        for byte in kind.iter().chain(payload) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB88320
                } else {
                    crc >> 1
                };
            }
        }
        png.extend_from_slice(&(!crc).to_be_bytes());
    }

    fn red_png() -> Vec<u8> {
        // PNG RGBA8, 1x1 red. Zlib contains one stored DEFLATE block for
        // filter=0, RGBA=255,0,0,255. Adler32=0x050001FF, independent of image.
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        png_chunk(
            &mut png,
            b"IDAT",
            &[
                0x78, 1, 1, 5, 0, 0xFA, 0xFF, 0, 255, 0, 0, 255, 5, 0, 1, 255,
            ],
        );
        png_chunk(&mut png, b"IEND", &[]);
        png
    }

    #[test]
    fn lua_image_independent_bmp_png_pixels_and_png_roundtrip() {
        let (lua, _) = runtime();
        lua.globals()
            .set("bmp", lua.create_buffer(BMP).unwrap())
            .unwrap();
        lua.globals()
            .set("png", lua.create_buffer(red_png()).unwrap())
            .unwrap();
        lua.load(
            r#"
            local bmp = ctx.image.decode(bmp)
            assert(bmp.width == 2 and bmp.height == 1)
            assert(buffer.tostring(bmp.rgba) == string.char(255, 0, 0, 255, 0, 255, 0, 255))
            local png = ctx.image.decode(png)
            assert(png.width == 1 and png.height == 1)
            assert(buffer.tostring(png.rgba) == string.char(255, 0, 0, 255))
            local encoded = ctx.image.png(bmp.width, bmp.height, bmp.rgba)
            assert(string.sub(buffer.tostring(encoded), 1, 8) == "\137PNG\r\n\26\n")
            local decoded = ctx.image.decode(encoded)
            assert(decoded.width == 2 and decoded.height == 1)
            assert(buffer.tostring(decoded.rgba) == buffer.tostring(bmp.rgba))
            local rgba = buffer.fromstring(string.char(7, 8, 9, 10))
            local alpha = ctx.image.decode(ctx.image.png(1, 1, rgba))
            assert(buffer.tostring(alpha.rgba) == buffer.tostring(rgba))
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn lua_image_rejects_invalid_dimensions_lengths_and_formats() {
        let (lua, _) = runtime();
        lua.load(r#"
            for _, dimensions in {{0,1}, {1,0}, {-1,1}, {1.5,1}, {0/0,1}, {math.huge,1}, {4294967296,1}, {4294967295,4294967295}, {"1",1}} do
                assert(not pcall(ctx.image.png, dimensions[1], dimensions[2], buffer.create(4)))
            end
            assert(not pcall(ctx.image.png, 1, 1, buffer.create(3)))
            assert(not pcall(ctx.image.png, 1, 1, buffer.create(5)))
            assert(not pcall(ctx.image.decode, buffer.fromstring("GIF89a\1\0\1\0")))
            assert(not pcall(ctx.image.decode, buffer.fromstring("\255\216\255\217")))
            assert(not pcall(ctx.image.decode, buffer.fromstring("BM")))
            assert(not pcall(ctx.image.decode, buffer.create(0)))
        "#).exec().unwrap();
        let mut excessive = BMP.to_vec();
        excessive[18..22].copy_from_slice(&16384_i32.to_le_bytes());
        excessive[22..26].copy_from_slice(&16384_i32.to_le_bytes());
        lua.globals()
            .set("oversized", lua.create_buffer(&excessive).unwrap())
            .unwrap();
        assert!(lua
            .load("return ctx.image.decode(oversized)")
            .eval::<Value>()
            .is_err());
        let mut corrupt = red_png();
        corrupt[45] ^= 1;
        lua.globals()
            .set("corrupt", lua.create_buffer(&corrupt).unwrap())
            .unwrap();
        assert!(lua
            .load("return ctx.image.decode(corrupt)")
            .eval::<Value>()
            .is_err());
    }

    #[test]
    fn lua_expanding_text_outputs_are_bounded() {
        let (lua, _) = runtime();
        lua.load(r#"
            local ok, err = pcall(ctx.encoding.encode, string.rep("x", 32 * 1024 * 1024 + 1), "utf16le")
            assert(not ok and string.find(tostring(err), "ASTRA_EMU_PATCH_BOUNDS", 1, true))
            local ok, err = pcall(ctx.toml.encode, {x = string.rep("\0", 12 * 1024 * 1024)})
            assert(not ok and string.find(tostring(err), "ASTRA_EMU_PATCH_BOUNDS", 1, true))
            local ok, err = pcall(ctx.json.encode, {x = string.rep("\0", 12 * 1024 * 1024)})
            assert(not ok and string.find(tostring(err), "ASTRA_EMU_PATCH_BOUNDS", 1, true))
        "#).exec().unwrap();
    }

    #[test]
    fn lua_toml_bounds_mature_parser_scratch_without_large_allocation() {
        let (lua, _) = runtime();
        // A long literal is one value, not one native node per text byte.
        lua.load(
            r#"
            local text = string.rep("x", 512 * 1024)
            assert(ctx.toml.decode('x = "' .. text .. '"').x == text)
        "#,
        )
        .exec()
        .unwrap();
        // Upstream into_vec reserves input byte count * Token size. The
        // string below is small; the prohibited >64MiB allocation never runs.
        let bytes = MAX_NATIVE_BYTES / std::mem::size_of::<toml_parser::lexer::Token>() + 1;
        let source = format!("x = '{}'", "x".repeat(bytes));
        lua.globals().set("source", source).unwrap();
        let error = lua
            .load("return ctx.toml.decode(source)")
            .eval::<Value>()
            .unwrap_err();
        assert!(error.to_string().contains("ASTRA_EMU_PATCH_MEMORY"));
        assert!(error.to_string().contains(&MAX_NATIVE_BYTES.to_string()));
    }

    #[test]
    fn lua_all_native_families_honor_cancellation() {
        let (lua, cancelled) = runtime();
        lua.globals()
            .set("png", lua.create_buffer(red_png()).unwrap())
            .unwrap();
        cancelled.store(true, Ordering::Release);
        lua.load(r#"
            local b = buffer.fromstring("abcd")
            local function cancelled(f, ...)
                local ok, err = pcall(f, ...)
                assert(not ok and string.find(tostring(err), "ASTRA_EMU_PATCH_CANCELLED", 1, true))
            end
            for _, hash in {ctx.hash.sha256, ctx.hash.md5, ctx.hash.crc32, ctx.hash.crc16_ibm} do cancelled(hash, b) end
            cancelled(ctx.encoding.decode, b, "utf8")
            cancelled(ctx.encoding.encode, "abc", "cp932")
            cancelled(ctx.json.decode, "{}")
            cancelled(ctx.json.encode, {})
            cancelled(ctx.toml.decode, "x = 1")
            cancelled(ctx.toml.encode, {})
            cancelled(ctx.image.decode, png)
            cancelled(ctx.image.png, 1, 1, b)
        "#).exec().unwrap();
    }
}
