-- Legacy MinoriPatch data only. Standard PAZ and script decoding remain native.
local M = {}
local MAX_RANGE = 16 * 1024 * 1024

local function fail(ctx, code, message)
    ctx.fail(code, message)
    error(message)
end

local function range(ctx, size, offset, length)
    if offset < 0 or length < 0 or offset % 1 ~= 0 or length % 1 ~= 0
        or offset > size or length > size - offset then
        fail(ctx, "BOUNDS", "Legacy localization range is outside its input")
    end
end

local function equal(a, b)
    return buffer.len(a) == buffer.len(b) and buffer.tostring(a) == buffer.tostring(b)
end

-- The native query ignores NUL bytes, wraps at 32 bits and clears bit 31.
function M.source_hash(bytes)
    local h = 0
    for i = 0, buffer.len(bytes) - 1 do
        local b = buffer.readu8(bytes, i)
        if b ~= 0 then
            h = bit32.band(h * 131 + b, 0x7fffffff)
        end
    end
    return h
end

function M.read(ctx, source)
    range(ctx, source:size(), 0, 16)
    local header = source:read(0, 16)
    local count = buffer.readu32(header, 0)
    local compression = buffer.readu32(header, 4)
    local stored = buffer.readu32(header, 8)
    local unpacked = buffer.readu32(header, 12)
    if compression ~= 0 then
        -- The shipped files use absolute file offsets. The DLL compressed branch
        -- does not establish a usable offset base; do not invent one.
        fail(ctx, "UNSUPPORTED_INPUT", "Compressed ACR offset identity is not established")
    end
    if stored ~= source:size() - 16 or unpacked ~= stored then
        fail(ctx, "INTEGRITY", "ACR header lengths disagree with its input")
    end
    if count > math.floor(stored / 20) or count * 20 > MAX_RANGE then
        fail(ctx, "BOUNDS", "ACR index exceeds its bounded input range")
    end
    local index = source:read(16, count * 20)
    local records = table.create(count)
    for i = 0, count - 1 do
        ctx.check_cancelled()
        local p = i * 20
        local hash = buffer.readu32(index, p)
        local from = buffer.readu32(index, p + 4)
        local from_size = buffer.readu32(index, p + 8)
        local to = buffer.readu32(index, p + 12)
        local to_size = buffer.readu32(index, p + 16)
        for _, item in {{from, from_size}, {to, to_size}} do
            range(ctx, source:size(), item[1], item[2])
            if item[1] < 16 + count * 20 or item[2] == 0 or item[2] > MAX_RANGE then
                fail(ctx, "BOUNDS", "ACR text overlaps its header/index or exceeds a bounded read")
            end
        end
        local original = source:read(from, from_size)
        local translated = source:read(to, to_size)
        if hash ~= M.source_hash(original) then
            fail(ctx, "INTEGRITY", "ACR source-byte identity hash does not match its index")
        end
        records[i + 1] = {source_hash = hash, source = original, replacement = translated}
    end
    return records
end

-- ACR stores the original raw bytes, so its own index establishes byte identity.
-- This does not establish which native display endpoints can produce those bytes.
-- Preserve every ordered row; the native lookup uses the first hash match, while
-- source_sha256 is audit data only, never a collision-disambiguation key.
function M.translation_table(ctx, names, dialogue, encodings)
    local function valid_encoding(value)
        return value == "cp932" or value == "gbk"
    end
    if type(encodings) ~= "table" or not valid_encoding(encodings.source)
        or not valid_encoding(encodings.names) or not valid_encoding(encodings.dialogue) then
        fail(ctx, "INVALID_REQUEST", "ACR encodings must follow each actual native consumer")
    end
    local audit = {duplicate_hash_rows = 0, different_identity_collisions = 0, differing_translation_rows = 0}
    local function convert(records, role)
        local seen = {}
        local output = table.create(#records)
        for index, record in records do
            ctx.check_cancelled()
            if record.source_hash ~= M.source_hash(record.source) then
                fail(ctx, "INTEGRITY", "ACR source bytes no longer match their index hash")
            end
            local previous = seen[record.source_hash]
            if previous then
                audit.duplicate_hash_rows += 1
                if not equal(previous.source, record.source) then
                    audit.different_identity_collisions += 1
                end
                if not equal(previous.replacement, record.replacement) then
                    audit.differing_translation_rows += 1
                end
            else
                seen[record.source_hash] = record
            end
            local text = ctx.encoding.decode(record.replacement, encodings[role])
            if string.find(text, "\0", 1, true) then
                fail(ctx, "INTEGRITY", "ACR translation contains an embedded terminator")
            end
            output[index] = {source_hash = record.source_hash,
                source_sha256 = ctx.hash.sha256(record.source), text = text}
        end
        return output
    end
    return {
        version = 2,
        source_encoding = encodings.source,
        hash_collision_policy = "first_record",
        names = convert(names, "names"),
        dialogue = convert(dialogue, "dialogue"),
    }, audit
end

-- cnbg is a separate custom index, not a PAZ archive. The DLL uses CP932
-- filenames and changes the requested .png suffix to .bmp for this lookup.
function M.cnbg_index(ctx, source)
    range(ctx, source:size(), 0, 24)
    local header = source:read(0, 24)
    if buffer.readstring(header, 0, 8) ~= "FUYINPAK"
        or buffer.readu32(header, 8) ~= 24 or buffer.readu32(header, 12) ~= 0 then
        fail(ctx, "UNSUPPORTED_INPUT", "cnbg header does not match its supported custom layout")
    end
    local offset = buffer.readu32(header, 16)
    local size = buffer.readu32(header, 20)
    range(ctx, source:size(), offset, size)
    if offset < 24 or size == 0 or size % 20 ~= 0 or size > MAX_RANGE
        or offset + size ~= source:size() then
        fail(ctx, "INTEGRITY", "cnbg index has invalid placement, stride or trailing bytes")
    end
    local index = source:read(offset, size)
    local entries = {}
    local seen = {}
    local spans = {}
    for p = 0, size - 1, 20 do
        ctx.check_cancelled()
        local name_at = buffer.readu32(index, p)
        local name_size = buffer.readu32(index, p + 4)
        local data_at = buffer.readu32(index, p + 8)
        local data_size = buffer.readu32(index, p + 12)
        range(ctx, source:size(), name_at, name_size)
        range(ctx, source:size(), data_at, data_size)
        if name_size == 0 or name_size > 260 or data_size < 54 or data_size > MAX_RANGE then
            fail(ctx, "BOUNDS", "cnbg name or bitmap exceeds its supported range")
        end
        if name_at < 24 or data_at < 24 or name_at + name_size > offset
            or data_at + data_size > offset or buffer.readu32(index, p + 16) ~= data_size then
            fail(ctx, "INTEGRITY", "cnbg payload overlaps metadata or disagrees with its stored size")
        end
        table.insert(spans, {name_at, name_at + name_size})
        table.insert(spans, {data_at, data_at + data_size})
        local name = ctx.encoding.decode(source:read(name_at, name_size), "cp932")
        name = string.gsub(name, "\0$", "")
        name = string.gsub(name, "\\", "/")
        if string.find(name, "\0", 1, true) or string.sub(name, -4) ~= ".bmp"
            or string.sub(name, 1, 1) == "/" or string.find(name, ":", 1, true) then
            fail(ctx, "INTEGRITY", "cnbg resource name is not a relative bitmap identity")
        end
        for component in string.gmatch(name, "[^/]+") do
            if component == "." or component == ".." then
                fail(ctx, "PATH", "cnbg resource name has a traversal component")
            end
        end
        local png = string.sub(name, 1, -5) .. ".png"
        if seen[string.lower(png)] then
            fail(ctx, "INTEGRITY", "cnbg resource names collide under native case rules")
        end
        seen[string.lower(png)] = true
        table.insert(entries, {name = name, resource = png, offset = data_at, size = data_size,
            value = buffer.readu32(index, p + 16)})
    end
    table.sort(spans, function(a, b) return a[1] < b[1] end)
    for i = 2, #spans do
        if spans[i][1] < spans[i - 1][2] then
            fail(ctx, "INTEGRITY", "cnbg names or bitmap payloads overlap")
        end
    end
    return entries
end

-- Preserve all four stored channels, including reserved A=0. Generic BI_RGB
-- decoders may replace that byte with 255; base-background opacity belongs to
-- the native composition endpoint, not to this offline resource normalizer.
function M.cnbg_png(ctx, source, entry, expected)
    range(ctx, source:size(), entry.offset, entry.size)
    if entry.size < 54 or entry.size > MAX_RANGE then
        fail(ctx, "BOUNDS", "cnbg bitmap exceeds its bounded input range")
    end
    local bmp = source:read(entry.offset, entry.size)
    if buffer.readu16(bmp, 0) ~= 0x4d42 or buffer.readu32(bmp, 14) ~= 40
        or buffer.readu32(bmp, 10) ~= 54 or buffer.readu16(bmp, 26) ~= 1
        or buffer.readu16(bmp, 28) ~= 32 or buffer.readu32(bmp, 30) ~= 0 then
        fail(ctx, "UNSUPPORTED_INPUT", "cnbg bitmap is not the native uncompressed 32-bit layout")
    end
    local width = buffer.readi32(bmp, 18)
    local signed_height = buffer.readi32(bmp, 22)
    local height = math.abs(signed_height)
    if width <= 0 or height == 0 or width * height > MAX_RANGE / 4 then
        fail(ctx, "BOUNDS", "cnbg bitmap dimensions exceed their bounded pixel range")
    end
    local pixels = width * height * 4
    local tail = entry.size - 54 - pixels
    if buffer.readu32(bmp, 2) ~= entry.size
        or buffer.readu32(bmp, 34) ~= pixels + tail
        or (tail ~= 0 and tail ~= 2)
        or (tail == 2 and buffer.readu16(bmp, entry.size - 2) ~= 0) then
        fail(ctx, "INTEGRITY", "cnbg bitmap pixel extent disagrees with its declared size")
    end
    if not expected or width ~= expected.width or height ~= expected.height then
        fail(ctx, "INTEGRITY", "cnbg bitmap dimensions disagree with the original resource")
    end
    -- Optional externally pinned payload CRC; the cnbg header has no CRC field.
    if expected.crc32 and ctx.hash.crc32(bmp) ~= expected.crc32 then
        fail(ctx, "INTEGRITY", "cnbg bitmap disagrees with its pinned payload CRC")
    end
    local rgba = buffer.create(pixels)
    for y = 0, height - 1 do
        ctx.check_cancelled()
        local row = if signed_height > 0 then height - 1 - y else y
        for x = 0, width - 1 do
            local input = 54 + (row * width + x) * 4
            local output = (y * width + x) * 4
            buffer.writeu8(rgba, output, buffer.readu8(bmp, input + 2))
            buffer.writeu8(rgba, output + 1, buffer.readu8(bmp, input + 1))
            buffer.writeu8(rgba, output + 2, buffer.readu8(bmp, input))
            buffer.writeu8(rgba, output + 3, buffer.readu8(bmp, input + 3))
        end
    end
    return ctx.image.png(width, height, rgba)
end

return M
