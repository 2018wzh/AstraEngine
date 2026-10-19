-- Offline Enigma format-3 VFS data reader. No protector instructions are executed.
-- Layout comes from the caller's verified version, not PE signatures or guesses.
-- This wire format has no block CRC field. Callers must authenticate their input
-- and independently verify decoded resources before publishing a conversion.
local M = {}
local BLOCK = 65536

local function reject(ctx, code, message)
    ctx.fail(code, "Enigma format3: " .. message)
end

function M.directory(ctx, source, layout)
    local binary = ctx.load("common/binary.luau")
    local r = binary.reader(ctx, source)
    r:range(layout.directory_offset, layout.directory_length)
    r:range(layout.payload_offset, layout.payload_length)
    if binary.add(layout.directory_offset, layout.directory_length) ~= layout.payload_offset then
        reject(ctx, "INTEGRITY", "directory and payload are not adjacent")
    end
    local cursor = layout.directory_offset
    local ending = layout.payload_offset
    local function take(length)
        local start = cursor
        cursor = binary.add(cursor, length)
        if cursor > ending then
            reject(ctx, "BOUNDS", "truncated directory record")
        end
        return start
    end
    local function u8() return r:u8(take(1)) end
    local function u32() return r:u32le(take(4)) end
    local function u64() return r:u64le(take(8)) end
    local function text()
        local start = cursor
        while true do
            local at = take(2)
            if r:u16le(at) == 0 then
                return ctx.encoding.decode(r:bytes(start, at - start), "utf16le")
            end
        end
    end
    local files, nodes, ids = {}, {}, {}
    local stack = {}
    local expected_payload = 0
    repeat
        ctx.check_cancelled()
        while #stack > 0 and stack[#stack].remaining == 0 do
            table.remove(stack)
        end
        local parent = if #stack > 0 then stack[#stack] else nil
        if parent then parent.remaining -= 1 end
        local id, level, children = u32(), u32(), u32()
        local name, kind = text(), u8()
        if ids[id] or level ~= #stack then
            reject(ctx, "INTEGRITY", "duplicate node identity or invalid tree level")
        end
        ids[id] = true
        if parent then
            if name == "" or parent.names[name] then
                reject(ctx, "INTEGRITY", "empty or duplicate sibling name")
            end
            parent.names[name] = true
        end
        if (#nodes == 0 and kind ~= 0) or (#nodes > 0 and kind == 0) then
            reject(ctx, "INTEGRITY", "invalid root node")
        end
        if children > math.floor((ending - cursor) / 15) then
            reject(ctx, "BOUNDS", "child count exceeds directory bounds")
        end
        local node = { id = id, level = level, name = name, kind = kind,
            parent = if parent then parent.node else nil }
        table.insert(nodes, node)
        if kind == 3 then
            node.mode = u8()
            take(28) -- Three f64 timestamps and u32 attributes, never interpreted.
            if node.mode ~= 0 then
                reject(ctx, "UNSUPPORTED_INPUT", "unsupported directory mode")
            end
        elseif kind == 2 then
            node.flags, node.mode, node.size = u8(), u8(), u64()
            take(28)
            node.extra, node.aux = u8(), text()
            node.offset, node.stored = u64(), u64()
            if children ~= 0 or node.flags ~= 0 or node.mode ~= 0
                or node.extra ~= 0 or node.aux ~= "" then
                reject(ctx, "UNSUPPORTED_INPUT", "unsupported file mode or auxiliary data")
            end
            if node.offset ~= expected_payload then
                reject(ctx, "INTEGRITY", "noncontiguous file payload")
            end
            expected_payload = binary.add(node.offset, node.stored)
            if expected_payload > layout.payload_length then
                reject(ctx, "BOUNDS", "file payload exceeds container")
            end
            local count = math.ceil(node.size / BLOCK)
            local table_length = count * 12
            if table_length > node.stored or (count == 0 and node.stored ~= 0) then
                reject(ctx, "BOUNDS", "block table exceeds file payload")
            end
            node.blocks = {}
            local next_offset = table_length
            local base = binary.add(layout.payload_offset, node.offset)
            for index = 0, count - 1 do
                ctx.check_cancelled()
                local at = binary.add(base, index * 12)
                local offset, length = r:u64le(at), r:u32le(at + 8)
                if offset ~= next_offset or length == 0 then
                    reject(ctx, "INTEGRITY", "noncontiguous or empty compressed block")
                end
                next_offset = binary.add(offset, length)
                if next_offset > node.stored then
                    reject(ctx, "BOUNDS", "compressed block exceeds file payload")
                end
                table.insert(node.blocks, { offset = binary.add(base, offset),
                    stored = length, size = math.min(BLOCK, node.size - index * BLOCK) })
            end
            if next_offset ~= node.stored then
                reject(ctx, "INTEGRITY", "trailing file payload")
            end
            table.insert(files, node)
        elseif kind ~= 0 then
            reject(ctx, "UNSUPPORTED_INPUT", "unsupported node type")
        end
        if children > 0 then
            table.insert(stack, { node = node, remaining = children, names = {} })
        end
        while #stack > 0 and stack[#stack].remaining == 0 do
            table.remove(stack)
        end
    until #stack == 0
    if cursor ~= ending or expected_payload ~= layout.payload_length then
        reject(ctx, "INTEGRITY", "directory or payload not consumed exactly")
    end
    return { reader = r, files = files, nodes = nodes }
end

-- A sink receives bounded decoded blocks and their logical offsets. It may write
-- standard native resources; directory macro names are never used as host paths.
function M.decode(ctx, archive, file, sink)
    local offset = 0
    for _, block in ipairs(file.blocks) do
        ctx.check_cancelled()
        local packed = archive.reader:bytes(block.offset, block.stored)
        local plain = ctx.codec.aplib(packed, block.size)
        if buffer.len(plain) ~= block.size then
            reject(ctx, "INTEGRITY", "decoded block length mismatch")
        end
        sink(offset, plain)
        offset += block.size
    end
    if offset ~= file.size then
        reject(ctx, "INTEGRITY", "decoded file length mismatch")
    end
end

return M
