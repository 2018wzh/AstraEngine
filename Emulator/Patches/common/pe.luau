-- Data-only PE inspection for necessary offline legacy packaging.
local pe = {}
local contexts = setmetatable({}, { __mode = "k" })
local RVA_END = 4294967296

local function need(ctx, condition, message)
    if not condition then ctx.fail("BOUNDS", message) end
end

local function rva_end(ctx, start, size)
    need(ctx, type(start) == "number" and type(size) == "number"
        and start >= 0 and start < RVA_END and start == math.floor(start)
        and size >= 0 and size <= RVA_END - start and size == math.floor(size), "invalid PE RVA range")
    return start + size
end

function pe.parse(ctx, source)
    local binary = ctx.load("common/binary.luau")
    local reader = binary.reader(ctx, source)
    reader:range(0, 64)
    need(ctx, buffer.tostring(reader:bytes(0, 2)) == "MZ", "missing DOS signature")
    local header = reader:u32le(60)
    need(ctx, header >= 64, "PE header overlaps DOS header")
    reader:range(header, 24)
    need(ctx, buffer.tostring(reader:bytes(header, 4)) == "PE\0\0", "missing PE signature")
    local machine = reader:u16le(header + 4)
    local count = reader:u16le(header + 6)
    local optional_size = reader:u16le(header + 20)
    local optional = binary.add(header, 24)
    reader:range(optional, optional_size)
    need(ctx, optional_size >= 2, "truncated PE optional header")
    local magic = reader:u16le(optional)
    local directory_offset, base
    if magic == 0x10b then
        directory_offset = 96
        need(ctx, optional_size >= directory_offset, "truncated PE32 optional header")
        base = reader:u32le(optional + 28)
    elseif magic == 0x20b then
        directory_offset = 112
        need(ctx, optional_size >= directory_offset, "truncated PE32+ optional header")
        base = reader:u64le(optional + 24)
    else
        ctx.fail("UNSUPPORTED_INPUT", "only PE32 and PE32+ packaging is supported")
    end
    local image_size = reader:u32le(optional + 56)
    local headers_size = reader:u32le(optional + 60)
    local table_offset = binary.add(optional, optional_size)
    local table_end = binary.add(table_offset, count * 40)
    need(ctx, table_end <= headers_size and headers_size <= image_size, "invalid PE header extent")
    reader:range(0, headers_size)
    local directory_count = reader:u32le(optional + directory_offset - 4)
    need(ctx, directory_count <= math.floor((optional_size - directory_offset) / 8), "truncated PE directories")
    local directories = {}
    for index = 1, directory_count do
        ctx.check_cancelled()
        local at = optional + directory_offset + (index - 1) * 8
        local rva, size = reader:u32le(at), reader:u32le(at + 4)
        if index == 5 then
            -- The certificate directory is a physical file range, never an RVA.
            reader:range(rva, size)
        else
            rva_end(ctx, rva, size)
        end
        directories[index] = { rva = rva, size = size }
    end
    local sections = {}
    local overlay = headers_size
    for index = 1, count do
        ctx.check_cancelled()
        local at = table_offset + (index - 1) * 40
        local raw_name = buffer.tostring(reader:bytes(at, 8))
        local nul = string.find(raw_name, "\0", 1, true)
        local name = if nul then string.sub(raw_name, 1, nul - 1) else raw_name
        local virtual_size, rva = reader:u32le(at + 8), reader:u32le(at + 12)
        local size, offset = reader:u32le(at + 16), reader:u32le(at + 20)
        reader:range(offset, size)
        need(ctx, rva_end(ctx, rva, math.max(virtual_size, size)) <= image_size, "PE section exceeds image")
        sections[index] = { name = name, rva = rva, virtual_size = virtual_size,
            offset = offset, size = size, flags = reader:u32le(at + 36) }
        overlay = math.max(overlay, binary.add(offset, size))
    end
    local parsed = { reader = reader, machine = machine, base = base, image_size = image_size,
        headers_size = headers_size, sections = sections, directories = directories, overlay = overlay }
    contexts[parsed] = ctx
    return parsed
end

function pe.offset(parsed, rva, length)
    local ctx = assert(contexts[parsed], "PE object must come from pe.parse")
    local ending = rva_end(ctx, rva, length)
    need(ctx, ending <= parsed.image_size, "PE RVA exceeds image")
    -- A zero-length query still needs a uniquely mapped start, not a BSS/end pointer.
    local probe_end = if length == 0 then rva + 1 else ending
    local mapped, intersections = nil, 0
    if rva < parsed.headers_size then
        intersections += 1
        if probe_end <= parsed.headers_size then mapped = rva end
    end
    for _, section in ipairs(parsed.sections) do
        ctx.check_cancelled()
        local section_end = section.rva + math.max(section.virtual_size, section.size)
        if rva < section_end and probe_end > section.rva then
            intersections += 1
            if rva >= section.rva and probe_end <= section.rva + section.size then
                mapped = section.offset + (rva - section.rva)
            end
        end
    end
    need(ctx, intersections == 1 and mapped ~= nil, "PE RVA is unmapped, unbacked, or overlapping")
    parsed.reader:range(mapped, length)
    return mapped
end

function pe.resources(ctx, parsed)
    local directory = parsed.directories[3]
    if directory == nil or (directory.rva == 0 and directory.size == 0) then return {} end
    need(ctx, directory.rva ~= 0 and directory.size >= 16, "invalid PE resource directory")
    local base = pe.offset(parsed, directory.rva, directory.size)
    local reader = parsed.reader
    local function relative(offset, size)
        need(ctx, offset <= directory.size and size <= directory.size - offset, "truncated PE resource metadata")
        return base + offset
    end
    local seen = {}
    local stack = {}
    local function push(offset, path)
        ctx.check_cancelled()
        need(ctx, not seen[offset], "repeated or cyclic PE resource directory")
        seen[offset] = true
        local at = relative(offset, 16)
        local named = reader:u16le(at + 12)
        local count = named + reader:u16le(at + 14)
        relative(offset + 16, count * 8)
        table.insert(stack, { at = at + 16, count = count, named = named, index = 0, path = path })
    end
    local result = {}
    push(0, {})
    while #stack > 0 do
        ctx.check_cancelled()
        local frame = stack[#stack]
        if frame.index == frame.count then
            table.remove(stack)
        else
            local at = frame.at + frame.index * 8
            local name, target = reader:u32le(at), reader:u32le(at + 4)
            local is_named = name >= 2147483648
            need(ctx, is_named == (frame.index < frame.named), "invalid PE named resource ordering")
            frame.index += 1
            local key = name
            if is_named then
                local relative_name = name - 2147483648
                local name_at = relative(relative_name, 2)
                local name_size = reader:u16le(name_at) * 2
                relative(relative_name + 2, name_size)
                key = ctx.encoding.decode(reader:bytes(name_at + 2, name_size), "utf16le")
            end
            local path = table.clone(frame.path)
            table.insert(path, key)
            if target >= 2147483648 then
                push(target - 2147483648, path)
            else
                local data = relative(target, 16)
                local data_rva, size = reader:u32le(data), reader:u32le(data + 4)
                local offset = pe.offset(parsed, data_rva, size)
                table.insert(result, { path = path, offset = offset, size = size,
                    codepage = reader:u32le(data + 8), data_rva = data_rva })
            end
        end
    end
    return result
end

return pe
