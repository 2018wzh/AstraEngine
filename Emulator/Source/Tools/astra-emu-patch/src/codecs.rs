//! Generic raw aPLib, zlib, and legacy cipher primitives for offline recipes.
//! Zlib consumes exactly one complete stream. Raw aPLib requires its terminator,
//! consumes every input byte, and ignores unused bits in its final tag byte.
//! Blowfish is big-endian ECB without padding; RC4 accepts 1..=256-byte keys.
use crate::{
    buffer::{self, BoundedWriter},
    error::check_cancelled,
    storage::scratch,
    PatchError, PatchErrorKind,
};
use blowfish::{
    cipher::{Block, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit, StreamCipher},
    Blowfish,
};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
use mlua::{Buffer, Lua, Table, Value};
use std::{
    io::Write,
    sync::{atomic::AtomicBool, Arc},
};

const CHUNK: usize = 1024 * 1024;
const WORK: usize = 32 * 1024;

pub(crate) fn install(lua: &Lua, ctx: &Table, cancelled: Arc<AtomicBool>) -> mlua::Result<()> {
    let codec = lua.create_table()?;
    let flag = cancelled.clone();
    codec.set(
        "inflate",
        lua.create_function(move |lua, (bytes, expected): (Buffer, Value)| {
            let expected = expected_size(expected)?;
            let result = buffer::input(lua, bytes, |bytes| inflate(bytes, expected, &flag))?;
            buffer::output(lua, &result)
        })?,
    )?;
    let flag = cancelled.clone();
    codec.set(
        "deflate",
        lua.create_function(move |lua, bytes: Buffer| {
            let result = buffer::input(lua, bytes, |bytes| deflate(bytes, &flag))?;
            buffer::output(lua, &result)
        })?,
    )?;
    let flag = cancelled.clone();
    codec.set(
        "aplib",
        lua.create_function(move |lua, (bytes, expected): (Buffer, Value)| {
            let expected = expected_size(expected)?;
            let result = buffer::input(lua, bytes, |bytes| aplib(bytes, expected, &flag))?;
            buffer::output(lua, &result)
        })?,
    )?;
    ctx.set("codec", codec)?;

    let crypto = lua.create_table()?;
    let flag = cancelled.clone();
    crypto.set(
        "blowfish",
        lua.create_function(move |lua, (bytes, key, decrypt): (Buffer, Buffer, bool)| {
            let result = buffer::input2(lua, bytes, key, |bytes, key| {
                blowfish(bytes, key, decrypt, &flag)
            })?;
            buffer::output(lua, &result)
        })?,
    )?;
    crypto.set(
        "rc4",
        lua.create_function(move |lua, (bytes, key, skip): (Buffer, Buffer, Value)| {
            let skip = buffer::integer(skip)?;
            let result = buffer::input2(lua, bytes, key, |bytes, key| {
                rc4(bytes, key, skip, &cancelled)
            })?;
            buffer::output(lua, &result)
        })?,
    )?;
    ctx.set("crypto", crypto)
}

fn expected_size(value: Value) -> mlua::Result<usize> {
    let value = buffer::integer(value)?;
    let size = usize::try_from(value).map_err(|cause| {
        mlua::Error::external(PatchError::with_source(
            PatchErrorKind::Bounds,
            "native length is not representable",
            cause,
        ))
    })?;
    buffer::bounded(size).map_err(mlua::Error::external)?;
    Ok(size)
}

fn codec_error(message: &'static str) -> PatchError {
    PatchError::new(PatchErrorKind::Codec, message)
}

fn inflate(bytes: &[u8], expected: usize, cancelled: &AtomicBool) -> Result<Vec<u8>, PatchError> {
    check_cancelled(cancelled)?;
    buffer::bounded(bytes.len())?;
    buffer::bounded(expected)?;
    let mut output = scratch(expected)?;
    let mut stream = Decompress::new(true);
    let mut probe = [0_u8; 1];
    loop {
        check_cancelled(cancelled)?;
        let before_in = stream.total_in() as usize;
        let before_out = stream.total_out() as usize;
        let end_in = bytes.len().min(before_in + CHUNK);
        // A one-byte probe lets the decoder consume the checksum at the exact
        // output boundary, while still detecting even one excess output byte.
        let target = if before_out == expected {
            &mut probe[..]
        } else {
            let end_out = expected.min(before_out + CHUNK);
            &mut output[before_out..end_out]
        };
        let status = stream
            .decompress(&bytes[before_in..end_in], target, FlushDecompress::None)
            .map_err(|cause| {
                PatchError::with_source(PatchErrorKind::Codec, "invalid zlib stream", cause)
            })?;
        let consumed = stream.total_in() as usize;
        let produced = stream.total_out() as usize;
        if produced > expected {
            return Err(codec_error("zlib output exceeds declared length"));
        }
        if status == Status::StreamEnd {
            if produced != expected || consumed != bytes.len() {
                return Err(codec_error("zlib length or trailing input mismatch"));
            }
            check_cancelled(cancelled)?;
            return Ok(output);
        }
        if consumed == before_in && produced == before_out {
            return Err(codec_error("incomplete zlib stream"));
        }
    }
}

fn deflate(bytes: &[u8], cancelled: &AtomicBool) -> Result<Vec<u8>, PatchError> {
    check_cancelled(cancelled)?;
    buffer::bounded(bytes.len())?;
    let mut stream = Compress::new(Compression::default(), true);
    let mut output = BoundedWriter::new();
    let mut work = [0_u8; WORK];
    loop {
        check_cancelled(cancelled)?;
        let before_in = stream.total_in() as usize;
        let before_out = stream.total_out();
        let end_in = bytes.len().min(before_in + CHUNK);
        let flush = if end_in == bytes.len() {
            FlushCompress::Finish
        } else {
            FlushCompress::None
        };
        let status = stream
            .compress(&bytes[before_in..end_in], &mut work, flush)
            .map_err(|cause| {
                PatchError::with_source(PatchErrorKind::Codec, "zlib compression failed", cause)
            })?;
        let produced = (stream.total_out() - before_out) as usize;
        output.write_all(&work[..produced]).map_err(|cause| {
            let kind = if cause.kind() == std::io::ErrorKind::OutOfMemory {
                PatchErrorKind::Memory
            } else if cause.kind() == std::io::ErrorKind::FileTooLarge {
                PatchErrorKind::Bounds
            } else {
                PatchErrorKind::Codec
            };
            PatchError::with_source(kind, "bounded zlib output failed", cause)
        })?;
        if status == Status::StreamEnd {
            if stream.total_in() as usize != bytes.len() {
                return Err(codec_error("zlib compression ended before input"));
            }
            check_cancelled(cancelled)?;
            return Ok(output.finish());
        }
        if stream.total_in() as usize == before_in && produced == 0 {
            return Err(codec_error("zlib compression made no progress"));
        }
    }
}

// The aPLib token format below is adapted from apultra's expand.c, with a new
// checked Rust reader, exact output allocation, and cancellation-aware copies.
// https://github.com/emmanuel-marty/apultra/blob/master/src/expand.c
//
// Copyright (C) 2019 Emmanuel Marty
//
// This software is provided 'as-is', without any express or implied
// warranty. In no event will the authors be held liable for any damages
// arising from the use of this software.
//
// Permission is granted to anyone to use this software for any purpose,
// including commercial applications, and to alter it and redistribute it
// freely, subject to the following restrictions:
//
// 1. The origin of this software must not be misrepresented; you must not
//    claim that you wrote the original software. If you use this software
//    in a product, an acknowledgment in the product documentation would be
//    appreciated but is not required.
// 2. Altered source versions must be plainly marked as such, and must not be
//    misrepresented as being the original software.
// 3. This notice may not be removed or altered from any source distribution.
struct AplibReader<'a> {
    bytes: &'a [u8],
    position: usize,
    tag: u8,
    mask: u8,
}

impl AplibReader<'_> {
    fn byte(&mut self) -> Result<u8, PatchError> {
        let byte = self
            .bytes
            .get(self.position)
            .copied()
            .ok_or_else(|| codec_error("incomplete aPLib stream"))?;
        self.position += 1;
        Ok(byte)
    }

    fn bit(&mut self) -> Result<usize, PatchError> {
        if self.mask == 0 {
            self.tag = self.byte()?;
            self.mask = 0x80;
        }
        let bit = usize::from(self.tag & self.mask != 0);
        self.mask >>= 1;
        Ok(bit)
    }

    fn gamma(&mut self) -> Result<usize, PatchError> {
        let mut value = 1_usize;
        loop {
            let bit = self.bit()?;
            value = value
                .checked_mul(2)
                .and_then(|value| value.checked_add(bit))
                .ok_or_else(|| codec_error("invalid aPLib gamma integer"))?;
            if self.bit()? == 0 {
                return Ok(value);
            }
        }
    }
}

fn aplib(bytes: &[u8], expected: usize, cancelled: &AtomicBool) -> Result<Vec<u8>, PatchError> {
    check_cancelled(cancelled)?;
    buffer::bounded(bytes.len())?;
    buffer::bounded(expected)?;
    if expected == 0 {
        return Err(codec_error("raw aPLib requires its first literal"));
    }
    let mut reader = AplibReader {
        bytes,
        position: 0,
        tag: 0,
        mask: 0,
    };
    let first = reader.byte()?;
    let mut output = scratch(expected)?;
    output[0] = first;
    let mut produced = 1;
    let mut last_offset = None;
    let mut follows_match = false;
    let mut commands = 0_u16;
    loop {
        // Even streams made entirely of literals or single-byte matches remain
        // cancellable; long matches additionally check each native copy chunk.
        if commands == 0 {
            check_cancelled(cancelled)?;
        }
        commands = (commands + 1) & 4095;
        if reader.bit()? == 0 {
            let literal = reader.byte()?;
            put_literal(&mut output, &mut produced, literal)?;
            follows_match = false;
        } else if reader.bit()? == 0 {
            let high = reader.gamma()?;
            let (offset, length) = if !follows_match && high == 2 {
                let offset = last_offset
                    .ok_or_else(|| codec_error("aPLib repeat has no previous offset"))?;
                (offset, reader.gamma()?)
            } else {
                let bias = if follows_match { 2 } else { 3 };
                let low = usize::from(reader.byte()?);
                let offset = high
                    .checked_sub(bias)
                    .and_then(|high| high.checked_mul(256))
                    .and_then(|high| high.checked_add(low))
                    .ok_or_else(|| codec_error("aPLib offset integer overflow"))?;
                let extra = if !(128..32000).contains(&offset) {
                    2
                } else if offset >= 1280 {
                    1
                } else {
                    0
                };
                let length = reader
                    .gamma()?
                    .checked_add(extra)
                    .ok_or_else(|| codec_error("aPLib length integer overflow"))?;
                last_offset = Some(offset);
                (offset, length)
            };
            copy_match(&mut output, &mut produced, offset, length, cancelled)?;
            follows_match = true;
        } else if reader.bit()? == 0 {
            let command = reader.byte()?;
            let offset = usize::from(command >> 1);
            if offset == 0 {
                // The original aPLib depacker ignores the short-form length bit
                // for offset zero, so both 0x00 and 0x01 terminate the stream.
                if produced != expected || reader.position != bytes.len() {
                    return Err(codec_error("aPLib length or trailing input mismatch"));
                }
                check_cancelled(cancelled)?;
                return Ok(output);
            }
            copy_match(
                &mut output,
                &mut produced,
                offset,
                2 + usize::from(command & 1),
                cancelled,
            )?;
            last_offset = Some(offset);
            follows_match = true;
        } else {
            let mut offset = 0;
            for _ in 0..4 {
                offset = offset * 2 + reader.bit()?;
            }
            if offset == 0 {
                put_literal(&mut output, &mut produced, 0)?;
            } else {
                copy_match(&mut output, &mut produced, offset, 1, cancelled)?;
            }
            follows_match = false;
        }
    }
}

fn put_literal(output: &mut [u8], produced: &mut usize, byte: u8) -> Result<(), PatchError> {
    let target = output
        .get_mut(*produced)
        .ok_or_else(|| codec_error("aPLib output exceeds declared length"))?;
    *target = byte;
    *produced += 1;
    Ok(())
}

fn copy_match(
    output: &mut [u8],
    produced: &mut usize,
    offset: usize,
    length: usize,
    cancelled: &AtomicBool,
) -> Result<(), PatchError> {
    if offset == 0 || offset > *produced {
        return Err(codec_error("invalid aPLib back-reference offset"));
    }
    let end = produced
        .checked_add(length)
        .filter(|end| *end <= output.len())
        .ok_or_else(|| codec_error("aPLib match exceeds declared length"))?;
    let start = *produced - offset;
    while *produced < end {
        check_cancelled(cancelled)?;
        // Double the initialized repeated prefix, preserving its phase when a
        // chunk boundary is not an exact multiple of the match offset.
        let source = start + (*produced - start) % offset;
        let count = (end - *produced).min(*produced - source).min(CHUNK);
        output.copy_within(source..source + count, *produced);
        *produced += count;
    }
    Ok(())
}

fn blowfish(
    bytes: &[u8],
    key: &[u8],
    decrypt: bool,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, PatchError> {
    check_cancelled(cancelled)?;
    buffer::bounded(bytes.len())?;
    if !bytes.len().is_multiple_of(8) {
        return Err(codec_error("Blowfish input must be 8-byte aligned"));
    }
    let cipher: Blowfish = Blowfish::new_from_slice(key).map_err(|cause| {
        PatchError::with_source(
            PatchErrorKind::Codec,
            "Blowfish key must contain 4 to 56 bytes",
            cause,
        )
    })?;
    let mut output = scratch(bytes.len())?;
    for (input, output) in bytes.chunks(CHUNK).zip(output.chunks_mut(CHUNK)) {
        check_cancelled(cancelled)?;
        for (input, output) in input
            .as_chunks::<8>()
            .0
            .iter()
            .zip(output.as_chunks_mut::<8>().0.iter_mut())
        {
            let input: &Block<Blowfish> = input.as_slice().try_into().expect("aligned block");
            let output: &mut Block<Blowfish> =
                output.as_mut_slice().try_into().expect("aligned block");
            if decrypt {
                cipher.decrypt_block_b2b(input, output);
            } else {
                cipher.encrypt_block_b2b(input, output);
            }
        }
    }
    check_cancelled(cancelled)?;
    Ok(output)
}

fn rc4(
    bytes: &[u8],
    key: &[u8],
    mut skip: u64,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, PatchError> {
    check_cancelled(cancelled)?;
    buffer::bounded(bytes.len())?;
    // rc4 0.2 overrides new_from_slice: its U32 default does not restrict this
    // constructor, which implements the full runtime 1..=256-byte key range.
    let mut cipher = rc4::Rc4::new_from_slice(key).map_err(|cause| {
        PatchError::with_source(
            PatchErrorKind::Codec,
            "RC4 key must contain 1 to 256 bytes",
            cause,
        )
    })?;
    let mut discard = [0_u8; WORK];
    while skip != 0 {
        check_cancelled(cancelled)?;
        let count = skip.min(WORK as u64) as usize;
        cipher
            .try_write_keystream(&mut discard[..count])
            .map_err(|cause| {
                PatchError::with_source(PatchErrorKind::Codec, "RC4 skip failed", cause)
            })?;
        skip -= count as u64;
    }
    check_cancelled(cancelled)?;
    let mut output = scratch(bytes.len())?;
    for (input, output) in bytes.chunks(CHUNK).zip(output.chunks_mut(CHUNK)) {
        check_cancelled(cancelled)?;
        cipher
            .try_apply_keystream_b2b(input, output)
            .map_err(|cause| {
                PatchError::with_source(PatchErrorKind::Codec, "RC4 transform failed", cause)
            })?;
    }
    check_cancelled(cancelled)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn vm() -> Lua {
        let lua = Lua::new();
        let ctx = lua.create_table().unwrap();
        install(&lua, &ctx, Arc::new(AtomicBool::new(false))).unwrap();
        lua.globals().set("ctx", ctx).unwrap();
        lua.load(
            r#"
            function hex(s)
                return (s:gsub('..', function(pair) return string.char(tonumber(pair, 16)) end))
            end
            function raw(b) return buffer.tostring(b) end
            function buf(s) return buffer.fromstring(s) end
        "#,
        )
        .exec()
        .unwrap();
        lua
    }

    #[test]
    fn blowfish_public_vectors_and_decryption() {
        // Eric Young's ECB vectors published by Bruce Schneier, including key setup.
        // https://www.schneier.com/code/vectors.txt
        vm().load(
            r#"
            local vectors = {
                {'0000000000000000', '0000000000000000', '4ef997456198dd78'},
                {'ffffffffffffffff', 'ffffffffffffffff', '51866fd5b85ecb8a'},
                {'f0e1d2c3', 'fedcba9876543210', 'be1e639408640f05'},
            }
            for _, v in vectors do
                local key = buf(hex(v[1]))
                local plain = hex(v[2])
                local encrypted = ctx.crypto.blowfish(buf(plain), key, false)
                assert(raw(encrypted) == hex(v[3]))
                assert(raw(ctx.crypto.blowfish(encrypted, key, true)) == plain)
            end
            for _, n in {4, 56} do
                local key = buf(string.rep('K', n))
                local encrypted = ctx.crypto.blowfish(buf('1234567812345678'), key, false)
                assert(raw(ctx.crypto.blowfish(encrypted, key, true)) == '1234567812345678')
                assert(raw(encrypted):sub(1, 8) == raw(encrypted):sub(9, 16))
            end
            assert(buffer.len(ctx.crypto.blowfish(buffer.create(0), buf('1234'), false)) == 0)
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn cipher_alignment_and_key_boundaries() {
        vm().load(
            r#"
            for _, n in {0, 1, 3, 57} do
                assert(not pcall(ctx.crypto.blowfish, buffer.create(8), buffer.create(n), false))
            end
            for _, n in {1, 7, 9, 15} do
                assert(not pcall(ctx.crypto.blowfish, buffer.create(n), buf('1234'), false))
            end
            for _, n in {0, 257} do
                assert(not pcall(ctx.crypto.rc4, buffer.create(8), buffer.create(n), 0))
            end
            for _, n in {1, 256} do
                local key = buffer.create(n)
                local cipher = ctx.crypto.rc4(buf('variable-key'), key, 7)
                assert(raw(ctx.crypto.rc4(cipher, key, 7)) == 'variable-key')
            end
            for _, n in {-1, 0.5, math.huge, 0/0, 9007199254740992, '0'} do
                assert(not pcall(ctx.crypto.rc4, buffer.create(1), buf('Key'), n))
            end
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn rc4_public_vectors_and_rfc6229_skip() {
        // RFC 6229, 40-bit key, offsets 0 and 256.
        vm().load(
            r#"
            local key = buf('Key')
            local cipher = ctx.crypto.rc4(buf('Plaintext'), key, 0)
            assert(raw(cipher) == hex('bbf316e8d940af0ad3'))
            assert(raw(ctx.crypto.rc4(cipher, key, 0)) == 'Plaintext')
            local rfc_key = buf(hex('0102030405'))
            assert(raw(ctx.crypto.rc4(buffer.create(16), rfc_key, 0)) ==
                hex('b2396305f03dc027ccc3524a0a1118a8'))
            assert(raw(ctx.crypto.rc4(buffer.create(16), rfc_key, 256)) ==
                hex('1cfcf62b03eddb641d77dfcf7f8d8c93'))
            local stream = raw(ctx.crypto.rc4(buffer.create(32785), rfc_key, 0))
            assert(raw(ctx.crypto.rc4(buffer.create(16), rfc_key, 32769)) == stream:sub(32770))
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn independent_zlib_bytes_and_exact_eof() {
        vm().load(
            r#"
            -- Fixed-Huffman and independently hand-assembled stored-block zlib.
            local fixed = hex('789ccb48cdc9c90700062c0215')
            local stored = hex('7801010500faff68656c6c6f062c0215')
            for _, s in {fixed, stored} do
                assert(raw(ctx.codec.inflate(buf(s), 5)) == 'hello')
                assert(not pcall(ctx.codec.inflate, buf(s), 4))
                assert(not pcall(ctx.codec.inflate, buf(s), 6))
                for n = 0, #s - 1 do
                    assert(not pcall(ctx.codec.inflate, buf(s:sub(1, n)), 5))
                end
                assert(not pcall(ctx.codec.inflate, buf(s .. '\0'), 5))
                assert(not pcall(ctx.codec.inflate, buf(s .. s), 5))
            end
            assert(raw(ctx.codec.inflate(buf(hex('789c030000000001')), 0)) == '')
            assert(not pcall(ctx.codec.inflate, buf(hex('789ccb48cdc9c90700062c0214')), 5))
            assert(not pcall(ctx.codec.inflate, buf(hex('cb48cdc9c90700')), 5)) -- raw DEFLATE
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn zlib_chunk_boundaries_and_declared_length_validation() {
        vm().load(
            r#"
            for _, n in {0, 1, 32768, 1048575, 1048576, 1048577, 2097161} do
                local data = string.rep('x', n)
                local packed = ctx.codec.deflate(buf(data))
                assert(raw(ctx.codec.inflate(packed, n)) == data)
                if n > 0 then assert(not pcall(ctx.codec.inflate, packed, n - 1)) end
            end
            for _, n in {-1, 1.5, math.huge, 0/0, 67108865, 9007199254740992, '5'} do
                assert(not pcall(ctx.codec.inflate, buffer.create(0), n))
                assert(not pcall(ctx.codec.aplib, buffer.create(0), n))
            end
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn aplib_handcrafted_literals_matches_and_repeat_state() {
        vm().load(r#"
            -- Tags are MSB first and interleaved with literal/offset bytes.
            local fixtures = {
                {'41c000', 'A'},                      -- first literal, EOF
                {'4130424300', 'ABC'},                 -- two verbatim literals
                {'41d80300', 'AAAA'},                  -- 110, offset 1, length 3
                {'41e38000', 'AA'},                    -- 111, four-bit offset 1
                {'41e18000', 'A\0'},                  -- 111, zero literal
                {'41a3010000', 'AAAAA'},               -- 10, gamma 3, offset 1, len 2+2
                {'41a101421600', 'AAAAABBBB'},          -- literal resets repeat state
                {'41d002016000', 'AAAAAAA'},            -- long match immediately after match
                {'416e4204706000', 'ABABBBB'},           -- tiny offset must not replace repeat offset
                {'41df00', 'A'},                      -- unused final tag bits need not be zero
                {'41c001', 'A'},                      -- original zero-offset EOF length bit
            }
            for _, fixture in fixtures do
                local packed, expected = hex(fixture[1]), fixture[2]
                assert(raw(ctx.codec.aplib(buf(packed), #expected)) == expected)
                assert(not pcall(ctx.codec.aplib, buf(packed), #expected - 1))
                assert(not pcall(ctx.codec.aplib, buf(packed), #expected + 1))
                assert(not pcall(ctx.codec.aplib, buf(packed .. '\0'), #expected))
                for n = 0, #packed - 1 do
                    assert(not pcall(ctx.codec.aplib, buf(packed:sub(1, n)), #expected))
                end
            end
        "#).exec().unwrap();
    }

    #[test]
    fn aplib_rejects_invalid_offsets_missing_eof_and_integer_overflow() {
        vm().load(
            r#"
            local offsets = {
                {'41d80400', 3},   -- short offset 2 before enough output
                {'41ff8000', 2},   -- tiny offset 15 before enough output
                {'41a3000000', 5}, -- long offset zero
                {'41a3020000', 5}, -- long offset 2 before enough output
            }
            for _, fixture in offsets do
                local ok, err = pcall(ctx.codec.aplib, buf(hex(fixture[1])), fixture[2])
                assert(not ok and string.find(tostring(err), 'back-reference offset', 1, true))
            end
            local ok, err = pcall(ctx.codec.aplib, buf(hex('41830000')), 3)
            assert(not ok and string.find(tostring(err), 'repeat has no previous offset', 1, true))
            ok, err = pcall(ctx.codec.aplib, buf(hex('41bf' .. string.rep('ff', 32))), 1)
            assert(not ok and string.find(tostring(err), 'gamma integer', 1, true))
            for _, s in {'4100', '41c0'} do
                assert(not pcall(ctx.codec.aplib, buf(hex(s)), 1))
            end
            assert(not pcall(ctx.codec.aplib, buf(''), 0))
            assert(not pcall(ctx.codec.aplib, buf(hex('41c000')), 0))
        "#,
        )
        .exec()
        .unwrap();
    }

    // Tiny independent bitstream assembler used only to exercise offset-size
    // thresholds and large overlapping matches, not to generate expected data.
    struct Fixture {
        bytes: Vec<u8>,
        tag: usize,
        used: u8,
    }
    impl Fixture {
        fn new(first: u8) -> Self {
            Self {
                bytes: vec![first],
                tag: 0,
                used: 8,
            }
        }
        fn bit(&mut self, bit: bool) {
            if self.used == 8 {
                self.tag = self.bytes.len();
                self.bytes.push(0);
                self.used = 0;
            }
            if bit {
                self.bytes[self.tag] |= 0x80 >> self.used;
            }
            self.used += 1;
        }
        fn gamma(&mut self, value: usize) {
            let top = usize::BITS - 1 - value.leading_zeros();
            for shift in (0..top).rev() {
                self.bit((value >> shift) & 1 != 0);
                self.bit(shift != 0);
            }
        }
        fn literal(&mut self, byte: u8) {
            self.bit(false);
            self.bytes.push(byte);
        }
        fn long(&mut self, offset: usize, gamma_length: usize) {
            self.bit(true);
            self.bit(false);
            self.gamma((offset >> 8) + 3); // follows a literal
            self.bytes.push(offset as u8);
            self.gamma(gamma_length);
        }
        fn finish(mut self) -> Vec<u8> {
            self.bit(true);
            self.bit(true);
            self.bit(false);
            self.bytes.push(0);
            self.bytes
        }
    }

    #[test]
    fn aplib_checked_offset_and_length_arithmetic() {
        let lua = vm();
        let mut offset = Fixture::new(b'A');
        offset.bit(true);
        offset.bit(false);
        offset.gamma(usize::MAX / 256 + 4);
        offset.bytes.push(0);
        offset.gamma(2);
        lua.globals()
            .set("packed", lua.create_buffer(offset.finish()).unwrap())
            .unwrap();
        lua.load(
            r#"
            local ok, err = pcall(ctx.codec.aplib, packed, 1)
            assert(not ok and string.find(tostring(err), 'offset integer overflow', 1, true))
        "#,
        )
        .exec()
        .unwrap();
        let mut length = Fixture::new(b'A');
        length.long(1, usize::MAX);
        lua.globals()
            .set("packed", lua.create_buffer(length.finish()).unwrap())
            .unwrap();
        lua.load(
            r#"
            local ok, err = pcall(ctx.codec.aplib, packed, 1)
            assert(not ok and string.find(tostring(err), 'length integer overflow', 1, true))
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn aplib_long_offset_thresholds_and_overlapping_chunk_copy() {
        let lua = vm();
        for (offset, extra) in [
            (127, 2),
            (128, 0),
            (1279, 0),
            (1280, 1),
            (31999, 1),
            (32000, 2),
        ] {
            let original: Vec<u8> = (0..offset).map(|i| (i % 251) as u8).collect();
            let mut packed = Fixture::new(original[0]);
            for &byte in &original[1..] {
                packed.literal(byte);
            }
            packed.long(offset, 2);
            let mut expected = original.clone();
            expected.extend_from_slice(&original[..2 + extra]);
            lua.globals()
                .set("packed", lua.create_buffer(packed.finish()).unwrap())
                .unwrap();
            lua.globals()
                .set("expected", lua.create_buffer(&expected).unwrap())
                .unwrap();
            lua.load("assert(raw(ctx.codec.aplib(packed, buffer.len(expected))) == raw(expected))")
                .exec()
                .unwrap();
        }
        let mut packed = Fixture::new(b'Z');
        packed.long(1, CHUNK + 11);
        lua.globals()
            .set("packed", lua.create_buffer(packed.finish()).unwrap())
            .unwrap();
        lua.load("assert(raw(ctx.codec.aplib(packed, 1048590)) == string.rep('Z', 1048590))")
            .exec()
            .unwrap();
        let mut packed = Fixture::new(b'X');
        packed.literal(b'Y');
        packed.literal(b'Z');
        packed.long(3, CHUNK + 11);
        lua.globals()
            .set("packed", lua.create_buffer(packed.finish()).unwrap())
            .unwrap();
        lua.load(
            r#"
            local n = 1048592
            local expected = (string.rep('XYZ', math.ceil(n / 3))):sub(1, n)
            assert(raw(ctx.codec.aplib(packed, n)) == expected)
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn all_primitives_observe_cancellation_without_lua_interrupts() {
        let lua = Lua::new();
        let flag = Arc::new(AtomicBool::new(false));
        let ctx = lua.create_table().unwrap();
        install(&lua, &ctx, flag.clone()).unwrap();
        lua.globals().set("ctx", ctx).unwrap();
        flag.store(true, Ordering::Release);
        lua.load(
            r#"
            local b = buffer.fromstring('12345678')
            local k = buffer.fromstring('1234')
            local calls = {
                function() ctx.codec.inflate(b, 8) end,
                function() ctx.codec.deflate(b) end,
                function() ctx.codec.aplib(b, 8) end,
                function() ctx.crypto.blowfish(b, k, false) end,
                function() ctx.crypto.rc4(b, k, 9007199254740991) end,
            }
            for _, f in calls do
                local ok, err = pcall(f)
                assert(not ok and string.find(tostring(err), 'ASTRA_EMU_PATCH_CANCELLED', 1, true))
            end
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn rc4_long_skip_is_cancellable_without_vm_scheduling() {
        let lua = Lua::new();
        let flag = Arc::new(AtomicBool::new(false));
        let ctx = lua.create_table().unwrap();
        install(&lua, &ctx, flag.clone()).unwrap();
        lua.globals().set("ctx", ctx).unwrap();
        let (send, receive) = std::sync::mpsc::sync_channel::<()>(0);
        lua.globals()
            .set(
                "begin_skip",
                lua.create_function(move |_, ()| {
                    send.send(()).unwrap();
                    Ok(())
                })
                .unwrap(),
            )
            .unwrap();
        let worker = std::thread::spawn(move || {
            receive.recv().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            flag.store(true, Ordering::Release);
        });
        let result = lua
            .load(
                r#"
            begin_skip()
            local ok, err = pcall(ctx.crypto.rc4, buffer.create(0),
                buffer.fromstring('Key'), 9007199254740991)
            assert(not ok and string.find(tostring(err), 'ASTRA_EMU_PATCH_CANCELLED', 1, true))
        "#,
            )
            .exec();
        worker.join().unwrap();
        result.unwrap();
    }
}
