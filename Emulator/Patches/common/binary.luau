-- Offline legacy packaging only; native engine formats use their native readers.
local binary = {}
local MAX = 9007199254740991
local READ_MAX = 16 * 1024 * 1024

local function integer(value)
    return type(value) == "number" and value >= 0 and value <= MAX and value == math.floor(value)
end

function binary.add(a, b)
    assert(integer(a) and integer(b) and a <= MAX - b, "binary integer addition overflow")
    return a + b
end

function binary.align(offset, alignment)
    assert(integer(offset) and integer(alignment) and alignment > 0, "invalid binary alignment")
    local remainder = offset % alignment
    return binary.add(offset, if remainder == 0 then 0 else alignment - remainder)
end

function binary.reader(ctx, source)
    local length = source:size()
    if not integer(length) then ctx.fail("BOUNDS", "file size exceeds exact integer range") end
    local reader = {}

    function reader:size()
        return length
    end

    function reader:range(offset, count)
        ctx.check_cancelled()
        if not integer(offset) or not integer(count) or offset > length or count > length - offset then
            ctx.fail("BOUNDS", "binary range exceeds input")
        end
    end

    function reader:bytes(offset, count)
        self:range(offset, count)
        if count > READ_MAX then ctx.fail("BOUNDS", "binary read exceeds 16 MiB") end
        return source:read(offset, count)
    end

    function reader:u8(offset)
        return buffer.readu8(self:bytes(offset, 1), 0)
    end

    function reader:u16le(offset)
        return buffer.readu16(self:bytes(offset, 2), 0)
    end

    function reader:u16be(offset)
        local bytes = self:bytes(offset, 2)
        return buffer.readu8(bytes, 0) * 256 + buffer.readu8(bytes, 1)
    end

    function reader:u32le(offset)
        return buffer.readu32(self:bytes(offset, 4), 0)
    end

    function reader:u32be(offset)
        local bytes = self:bytes(offset, 4)
        return buffer.readu8(bytes, 0) * 16777216 + buffer.readu8(bytes, 1) * 65536
            + buffer.readu8(bytes, 2) * 256 + buffer.readu8(bytes, 3)
    end

    function reader:i32le(offset)
        return buffer.readi32(self:bytes(offset, 4), 0)
    end

    function reader:u64le(offset)
        local bytes = self:bytes(offset, 8)
        local high = buffer.readu32(bytes, 4)
        if high > 2097151 then ctx.fail("BOUNDS", "u64 exceeds exact integer range") end
        return high * 4294967296 + buffer.readu32(bytes, 0)
    end

    function reader:cstring(offset, max_bytes)
        self:range(offset, 0)
        if not integer(max_bytes) or max_bytes > READ_MAX then
            ctx.fail("BOUNDS", "invalid bounded string length")
        end
        local available = math.min(max_bytes, length - offset)
        local scanned = 0
        while scanned < available do
            ctx.check_cancelled()
            local count = math.min(65536, available - scanned)
            local bytes = self:bytes(offset + scanned, count)
            for index = 0, count - 1 do
                if index % 4096 == 0 then ctx.check_cancelled() end
                if buffer.readu8(bytes, index) == 0 then
                    return self:bytes(offset, scanned + index)
                end
            end
            scanned += count
        end
        ctx.fail("BOUNDS", "bounded string has no terminator")
    end

    return reader
end

return binary
