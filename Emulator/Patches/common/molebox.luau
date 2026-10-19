-- Offline legacy packaging only; never execute the loader or expose a runtime VFS.
local M = {}
local MAX_RANGE = 16 * 1024 * 1024
local BLOCK_SIZE = 49152
local SCHEDULE = {11,12,13,14,8,9,10,4,5,6,7,1,2,3,12,13,14,0,9,10,11,5,6,7,8,2,3,4,13,14,0,1,10,11,12,6,7,8,9,3,4,5,14,0,1,2,11,12,13,7,8,9,10,4,5,6,0,1,2,3}

local function fail(ctx, code, message)
    ctx.fail(code, message)
    error(message)
end

local function bounded(ctx, size, offset, length)
    if offset < 0 or length < 0 or offset % 1 ~= 0 or length % 1 ~= 0
        or offset > size or length > size - offset then
        fail(ctx, "BOUNDS", "Molebox data range is outside its input")
    end
end

local function slice(ctx, bytes, offset, length)
    bounded(ctx, buffer.len(bytes), offset, length)
    local out = buffer.create(length)
    buffer.copy(out, 0, bytes, offset, length)
    return out
end

local function cstring(ctx, bytes, offset, limit)
    bounded(ctx, buffer.len(bytes), offset, 1)
    for i = offset, math.min(buffer.len(bytes), offset + limit) - 1 do
        if buffer.readu8(bytes, i) == 0 then
            return slice(ctx, bytes, offset, i - offset)
        end
    end
    fail(ctx, "INTEGRITY", "Molebox metadata string is not terminated within its bound")
end

local function md5_bytes(ctx, bytes)
    local hex = ctx.hash.md5(bytes)
    local digest = buffer.create(16)
    for i = 0, 15 do
        buffer.writeu8(digest, i, tonumber(string.sub(hex, i * 2 + 1, i * 2 + 2), 16))
    end
    return digest
end

-- Seed bytes and substitution data come from the selected loader/catalog.
-- Neither is a game-specific constant in this module.
function M.decode(ctx, bytes, seed, substitution, cbc)
    if buffer.len(seed) ~= 16 or buffer.len(substitution) ~= 256 or buffer.len(bytes) > MAX_RANGE then
        fail(ctx, "BOUNDS", "Molebox cipher parameters or data length are invalid")
    end
    local key = table.create(60)
    for i, index in SCHEDULE do
        key[i] = buffer.readu8(seed, index)
    end
    local result = slice(ctx, bytes, 0, buffer.len(bytes))
    local previous = table.create(8, 0)
    local p = table.create(8, 0)
    local ciphertext = table.create(8, 0)
    local function subst(value)
        return buffer.readu8(substitution, value)
    end
    for offset = 0, buffer.len(bytes) - 8, 8 do
        if offset % 4096 == 0 then ctx.check_cancelled() end
        for i = 1, 8 do
            p[i] = buffer.readu8(bytes, offset + i - 1)
            ciphertext[i] = p[i]
        end
        for round = 0, 7 do
            local k = round * 7
            p[5] = bit32.bxor(p[5], subst(bit32.bxor(p[1], key[k + 1])))
            p[6] = bit32.bxor(p[6], subst(bit32.bxor(p[2], key[k + 2])))
            p[7] = bit32.bxor(p[7], subst(bit32.bxor(p[3], key[k + 3])))
            p[8] = bit32.bxor(p[8], subst(bit32.bxor(p[4], key[k + 4])))
            p[2] = bit32.bxor(p[2], subst(bit32.bxor(p[5], key[k + 5])))
            p[3] = bit32.bxor(p[3], subst(bit32.bxor(p[5], p[6])))
            p[4] = bit32.bxor(p[4], subst(bit32.bxor(p[7], key[k + 6])))
            p[1] = bit32.bxor(p[1], subst(bit32.bxor(p[8], key[k + 7])))
        end
        for i = 1, 4 do
            p[i + 4] = bit32.bxor(p[i + 4], subst(bit32.bxor(p[i], key[56 + i])))
        end
        for i = 1, 8 do
            buffer.writeu8(result, offset + i - 1, cbc and bit32.bxor(p[i], previous[i]) or p[i])
            previous[i] = ciphertext[i]
        end
    end
    -- Native format transforms only floor(length/8) blocks; tail bytes stay raw.
    return result
end

function M.loader_lz(ctx, coded, expected)
    if expected <= 0 or expected > MAX_RANGE then
        fail(ctx, "BOUNDS", "Molebox loader declaration exceeds its bounded data size")
    end
    local output = buffer.create(expected)
    local input = 0
    local written = 0
    while written < expected do
        ctx.check_cancelled()
        bounded(ctx, buffer.len(coded), input, 2)
        local tag = buffer.readu8(coded, input)
        if tag == 0x80 then
            buffer.writeu8(output, written, buffer.readu8(coded, input + 1))
            written += 1
            input += 2
        elseif tag == 0 then
            local count = buffer.readu8(coded, input + 1) + 1
            bounded(ctx, buffer.len(coded), input + 2, count)
            bounded(ctx, expected, written, count)
            buffer.copy(output, written, coded, input + 2, count)
            input += count + 2
            written += count
        else
            local word = buffer.readu16(coded, input)
            local count = bit32.band(word, 15)
            local distance = bit32.rshift(word, 4) + 15
            bounded(ctx, written, written - distance, count)
            bounded(ctx, expected, written, count)
            buffer.copy(output, written, output, written - distance, count)
            input += 2
            written += count
        end
    end
    return output, input
end

-- marker_offset and substitution_offset are physical offsets established by
-- PE parsing plus the selected wrapper's static code, not virtual addresses.
function M.recover_loader(ctx, source, marker_offset, substitution_offset)
    local reader = ctx.load("common/binary.luau").reader(ctx, source)
    reader:range(marker_offset, 504)
    if bit32.bxor(reader:u32le(marker_offset), reader:u32le(marker_offset + 4)) ~= 520359445 then
        fail(ctx, "UNSUPPORTED_INPUT", "Molebox loader marker does not match its selected wrapper")
    end
    local source_table = reader:bytes(substitution_offset, 256)
    local substitution = buffer.create(256)
    for i = 0, 255 do
        buffer.writeu8(substitution, i, bit32.band(bit32.rshift(0xf563f082, (i + 1) % 21) - buffer.readu8(source_table, i), 255))
    end
    local raw = reader:bytes(marker_offset + 8, 496)
    local seed = slice(ctx, raw, 368, 16)
    local header = buffer.create(496)
    buffer.copy(header, 0, M.decode(ctx, slice(ctx, raw, 0, 368), seed, substitution, true))
    buffer.copy(header, 368, seed)
    buffer.copy(header, 384, M.decode(ctx, slice(ctx, raw, 384, 112), seed, substitution, true))
    local offset = buffer.readu32(header, 32)
    local length = buffer.readu32(header, 36)
    if length < 4 or length > MAX_RANGE then
        fail(ctx, "BOUNDS", "Molebox loader payload length is invalid")
    end
    local coded = M.decode(ctx, reader:bytes(offset, length), seed, substitution, true)
    local loader, consumed = M.loader_lz(ctx, slice(ctx, coded, 4, length - 4), buffer.readu32(coded, 0))
    if buffer.tostring(slice(ctx, loader, 0, 8)) ~= "STEELBOX" then
        fail(ctx, "INTEGRITY", "Recovered loader data does not have the STEELBOX header")
    end
    return {bytes = loader, header = header, trailing = length - 4 - consumed, substitution = substitution}
end

-- Offsets are established by the selected loader's static data references.
-- Read its built-in label rather than embedding a derived private seed.
function M.loader_parameters(ctx, recovered, label_offset, substitution_offset)
    local substitution = slice(ctx, recovered.bytes, substitution_offset, 256)
    if buffer.tostring(substitution) ~= buffer.tostring(recovered.substitution) then
        fail(ctx, "INTEGRITY", "Recovered loader substitution data disagrees with its wrapper")
    end
    return {
        seed = md5_bytes(ctx, cstring(ctx, recovered.bytes, label_offset, 4096)),
        substitution = substitution,
    }
end

function M.catalog(ctx, source, seed, substitution)
    local reader = ctx.load("common/binary.luau").reader(ctx, source)
    local trailer = M.decode(ctx, reader:bytes(reader:size() - 8, 8), seed, substitution, false)
    -- Trailer is LENGTH, CRC32 (not the opposite order).
    local length = buffer.readu32(trailer, 0)
    local crc = buffer.readu32(trailer, 4)
    if length < 16 or length > 65536 then
        fail(ctx, "INTEGRITY", "Molebox catalog length is invalid")
    end
    local data = M.decode(ctx, reader:bytes(reader:size() - 8 - length, length), seed, substitution, true)
    if ctx.hash.crc32(data) ~= crc then
        fail(ctx, "INTEGRITY", "Molebox catalog CRC32 does not match")
    end
    local offset = buffer.readu32(data, 8)
    local count = buffer.readu32(data, 12)
    bounded(ctx, length, offset, count * 24)
    local entries = table.create(count)
    for i = 0, count - 1 do
        local at = offset + i * 24
        local name = ctx.encoding.decode(cstring(ctx, data, buffer.readu32(data, at + 4), 4096), "utf8")
        entries[i + 1] = {flags = buffer.readu32(data, at), name = name, seed = slice(ctx, data, at + 8, 16)}
    end
    return entries
end

function M.open(ctx, source, seed, substitution)
    local reader = ctx.load("common/binary.luau").reader(ctx, source)
    local header_offset = reader:u32le(0)
    local header = M.decode(ctx, reader:bytes(header_offset, 48), seed, substitution, true)
    if buffer.tostring(slice(ctx, header, 40, 8)) ~= "STELPACK" then
        fail(ctx, "UNSUPPORTED_INPUT", "MYS does not use the selected CBC STELPACK format")
    end
    local index_length = buffer.readu32(header, 4)
    if index_length < 52 or index_length > MAX_RANGE then
        fail(ctx, "BOUNDS", "STELPACK index length exceeds its bounded range")
    end
    local index_offset = header_offset - index_length
    local index = M.decode(ctx, reader:bytes(index_offset, index_length), seed, substitution, true)
    if ctx.hash.crc32(index) ~= buffer.readu32(header, 16) then
        fail(ctx, "INTEGRITY", "STELPACK index CRC32 does not match")
    end
    local base = header_offset - buffer.readu32(header, 12)
    reader:range(base, 0)
    local root_offset = buffer.readu32(header, 8)
    bounded(ctx, index_length, root_offset, 52)
    local root_flags = buffer.readu32(index, root_offset + 40)
    local count = buffer.readu32(index, root_offset + 48)
    if root_flags ~= 0x80000000 then
        fail(ctx, "UNSUPPORTED_INPUT", "STELPACK root is not the selected directory layout")
    end
    bounded(ctx, index_length, root_offset + 52, count * 4)
    local lookup = buffer.readu32(header, 20)
    local lookup_count = buffer.readu32(header, 24)
    bounded(ctx, index_length, lookup, lookup_count * 4)
    if lookup_count ~= count then
        fail(ctx, "INTEGRITY", "STELPACK root and lookup node counts disagree")
    end
    local nodes = table.create(count)
    local seen = {}
    local hashes = {}
    local block_count = 0
    for i = 0, count - 1 do
        ctx.check_cancelled()
        local offset = buffer.readu32(index, root_offset + 52 + i * 4)
        bounded(ctx, index_length, offset, 52)
        if seen[offset] then fail(ctx, "INTEGRITY", "STELPACK root repeats a node") end
        seen[offset] = true
        local flags = buffer.readu32(index, offset + 40)
        local logical = buffer.readu32(index, offset + 44)
        local blocks = buffer.readu32(index, offset + 48)
        if flags ~= 7 or buffer.readu32(index, offset + 8) ~= 0 or buffer.readu32(index, offset + 12) ~= 0 then
            fail(ctx, "UNSUPPORTED_INPUT", "STELPACK member is not the selected stripped-name file layout")
        end
        if blocks ~= math.ceil(logical / BLOCK_SIZE) then
            fail(ctx, "INTEGRITY", "STELPACK block count disagrees with its logical size")
        end
        bounded(ctx, index_length, offset + 52, blocks * 12)
        local identity = buffer.tostring(slice(ctx, index, offset + 20, 16))
        if hashes[identity] then fail(ctx, "INTEGRITY", "STELPACK contains duplicate path identities") end
        local descriptors = table.create(blocks)
        local summed = 0
        for j = 0, blocks - 1 do
            local at = offset + 52 + j * 12
            local coded = buffer.readu16(index, at)
            local decoded = buffer.readu16(index, at + 2)
            local block_flags = buffer.readu16(index, at + 4)
            local input = base + buffer.readu32(index, at + 8)
            if coded == 0 or decoded ~= math.min(BLOCK_SIZE, logical - summed) or bit32.band(block_flags, 0xfff9) ~= 0 then
                fail(ctx, "INTEGRITY", "STELPACK block flags or partition are invalid")
            end
            reader:range(input, coded)
            if input < base or input + coded > index_offset then
                fail(ctx, "BOUNDS", "STELPACK member block overlaps its index or header")
            end
            descriptors[j + 1] = {offset = input, coded = coded, decoded = decoded, flags = block_flags,
                crc16 = buffer.readu16(index, at + 6)}
            summed += decoded
        end
        if summed ~= logical then fail(ctx, "INTEGRITY", "STELPACK decoded lengths do not sum to member size") end
        local node = {offset = offset, identity = identity, size = logical, blocks = descriptors}
        nodes[i + 1] = node
        hashes[identity] = node
        block_count += blocks
    end
    local looked_up = {}
    for i = 0, lookup_count - 1 do
        local offset = buffer.readu32(index, lookup + i * 4)
        if not seen[offset] or looked_up[offset] then
            fail(ctx, "INTEGRITY", "STELPACK lookup does not contain the exact root node set")
        end
        looked_up[offset] = true
    end
    return {reader = reader, seed = seed, substitution = substitution, nodes = nodes,
        hashes = hashes, block_count = block_count}
end

-- Caller provides the exact uppercased normalized UTF16LE path bytes from the
-- native resource inventory/actual DLL arguments. No Unicode case-fold guess.
function M.bind_paths(ctx, package, candidates)
    local bound = {}
    for _, candidate in candidates do
        local digest = md5_bytes(ctx, candidate.uppercase_utf16le)
        local node = package.hashes[buffer.tostring(digest)]
        if node then
            if bound[node] and bound[node].path ~= candidate.path then
                fail(ctx, "INTEGRITY", "Different original paths match one STELPACK path identity")
            end
            bound[node] = candidate
        end
    end
    for _, node in package.nodes do
        if not bound[node] then
            fail(ctx, "UNSUPPORTED_INPUT", "STELPACK member still has no proven original path and consumer")
        end
    end
    return bound
end

function M.read_member(ctx, package, node, consume)
    local offset = 0
    for _, descriptor in node.blocks do
        ctx.check_cancelled()
        local bytes = package.reader:bytes(descriptor.offset, descriptor.coded)
        if bit32.band(descriptor.flags, 4) ~= 0 then
            bytes = M.decode(ctx, bytes, package.seed, package.substitution, true)
        end
        if bit32.band(descriptor.flags, 2) ~= 0 then
            local decoded, consumed = ctx.codec.inflate_prefix(bytes, descriptor.decoded)
            local padding = buffer.len(bytes) - consumed
            if padding > 7 or (padding ~= 0 and buffer.len(bytes) % 8 ~= 0) then
                fail(ctx, "INTEGRITY", "STELPACK compressed padding exceeds its cipher block")
            end
            bytes = decoded
        elseif buffer.len(bytes) ~= descriptor.decoded then
            fail(ctx, "INTEGRITY", "Uncompressed STELPACK block length disagrees with its descriptor")
        end
        if ctx.hash.crc16_ibm(bytes) ~= descriptor.crc16 then
            fail(ctx, "INTEGRITY", "STELPACK decoded block CRC16 does not match")
        end
        consume(offset, bytes)
        offset += buffer.len(bytes)
    end
    if offset ~= node.size then fail(ctx, "INTEGRITY", "STELPACK member length disagrees with its index") end
end

-- The 22 sys replacements are bytewise BGRA XOR deltas, not BMP images.
-- In particular, a BMP decoder may turn their reserved zero alpha into 255;
-- decoding the delta as an image would destroy every original alpha channel.
function M.xor_bitmap_png(ctx, bitmap, original_png)
    bounded(ctx, buffer.len(bitmap), 0, 54)
    if buffer.readu16(bitmap, 0) ~= 0x4d42 or buffer.readu32(bitmap, 2) ~= 0
        or buffer.readu32(bitmap, 10) ~= 54 or buffer.readu32(bitmap, 14) ~= 40
        or buffer.readu16(bitmap, 26) ~= 1 or buffer.readu16(bitmap, 28) ~= 32
        or buffer.readu32(bitmap, 30) ~= 0 then
        fail(ctx, "UNSUPPORTED_INPUT", "Molebox bitmap is not the proven 32-bit XOR variant")
    end
    local width = buffer.readi32(bitmap, 18)
    local signed_height = buffer.readi32(bitmap, 22)
    local height = math.abs(signed_height)
    if width <= 0 or height == 0 or width * height > MAX_RANGE / 4 then
        fail(ctx, "BOUNDS", "Molebox XOR bitmap dimensions exceed their bounded pixel range")
    end
    if buffer.len(bitmap) ~= 54 + width * height * 4 then
        fail(ctx, "INTEGRITY", "Molebox XOR bitmap pixel extent disagrees with its member size")
    end
    local original = ctx.image.decode(original_png)
    if original.width ~= width or original.height ~= height then
        fail(ctx, "INTEGRITY", "Molebox XOR bitmap dimensions disagree with the matching original")
    end
    for y = 0, height - 1 do
        ctx.check_cancelled()
        local row = signed_height > 0 and height - y - 1 or y
        for x = 0, width - 1 do
            local input = 54 + (row * width + x) * 4
            local output = (y * width + x) * 4
            for channel = 0, 3 do
                local source_channel = channel == 0 and 2 or channel == 2 and 0 or channel
                buffer.writeu8(original.rgba, output + channel,
                    bit32.bxor(buffer.readu8(original.rgba, output + channel),
                        buffer.readu8(bitmap, input + source_channel)))
            end
        end
    end
    return ctx.image.png(width, height, original.rgba)
end

return M
