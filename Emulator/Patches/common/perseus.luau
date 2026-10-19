-- Offline STELPACK restoration. Native Musica owns PAZ discovery, SC and playback.
local M = {}
local LIMIT = 16 * 1024 * 1024

local function fail(ctx, code, message)
    ctx.fail(code, message)
    error(message)
end

local function pinned(ctx, root, path, hash)
    local source = ctx.open(root, path)
    if source:sha256() ~= hash then
        fail(ctx, "UNSUPPORTED_INPUT", "Perseus input fingerprint does not match its selected edition")
    end
    return source
end

local function write(ctx, path, bytes)
    local parent = string.match(path, "^(.*)/[^/]+$")
    if parent then ctx.mkdir(parent) end
    local output = ctx.create(path)
    output:write(0, bytes)
    output:close()
end

local function prefix(role, path)
    if role == "musica_script" then return "localized/perseus/scripts/" .. path end
    if role == "font" then return "localized/perseus/fonts/" .. path end
    if role == "ass" then return "localized/perseus/subtitles/" .. path end
    return "localized/perseus/textures/" .. path
end

-- Full replacement BMPs contain meaningful reserved alpha, including zero.
-- ctx.image.decode(BMP) would apply BI_RGB's opaque-alpha convention instead.
function M.bitmap_png(ctx, bytes)
    if buffer.len(bytes) < 54 or buffer.readu16(bytes, 0) ~= 0x4d42
        or buffer.readu32(bytes, 2) ~= buffer.len(bytes) or buffer.readu32(bytes, 6) ~= 0
        or buffer.readu32(bytes, 10) ~= 54
        or buffer.readu32(bytes, 14) ~= 40 or buffer.readu16(bytes, 26) ~= 1
        or buffer.readu16(bytes, 28) ~= 32 or buffer.readu32(bytes, 30) ~= 0 then
        fail(ctx, "UNSUPPORTED_INPUT", "Perseus replacement is not the selected raw BGRA bitmap")
    end
    local width = buffer.readi32(bytes, 18)
    local signed_height = buffer.readi32(bytes, 22)
    local height = math.abs(signed_height)
    if width <= 0 or height == 0 or width * height > LIMIT / 4 then
        fail(ctx, "BOUNDS", "Perseus replacement dimensions exceed the bounded pixel extent")
    end
    local pixels = width * height * 4
    local tail = buffer.len(bytes) - 54 - pixels
    local size_image = buffer.readu32(bytes, 34)
    if (tail ~= 0 and tail ~= 2)
        or (tail == 2 and buffer.readu16(bytes, buffer.len(bytes) - 2) ~= 0)
        or (size_image ~= 0 and size_image ~= pixels + tail) then
        fail(ctx, "INTEGRITY", "Perseus replacement bitmap extent does not match its dimensions")
    end
    local rgba = buffer.create(width * height * 4)
    for y = 0, height - 1 do
        ctx.check_cancelled()
        local row = signed_height > 0 and height - y - 1 or y
        for x = 0, width - 1 do
            local input = 54 + (row * width + x) * 4
            local output = (y * width + x) * 4
            buffer.writeu8(rgba, output, buffer.readu8(bytes, input + 2))
            buffer.writeu8(rgba, output + 1, buffer.readu8(bytes, input + 1))
            buffer.writeu8(rgba, output + 2, buffer.readu8(bytes, input))
            buffer.writeu8(rgba, output + 3, buffer.readu8(bytes, input + 3))
        end
    end
    return ctx.image.png(width, height, rgba)
end

local function hex(ctx, text)
    if type(text) ~= "string" or #text % 2 ~= 0 or string.find(text, "[^%x]") then
        fail(ctx, "INTEGRITY", "Perseus private profile has invalid hexadecimal key data")
    end
    local bytes = buffer.create(#text / 2)
    for i = 0, buffer.len(bytes) - 1 do
        buffer.writeu8(bytes, i, tonumber(string.sub(text, i * 2 + 1, i * 2 + 2), 16))
    end
    return bytes
end

local function swap_words(ctx, bytes)
    for offset = 0, buffer.len(bytes) - 1, 4 do
        if offset % 4096 == 0 then ctx.check_cancelled() end
        local a, b = buffer.readu8(bytes, offset), buffer.readu8(bytes, offset + 1)
        buffer.writeu8(bytes, offset, buffer.readu8(bytes, offset + 3))
        buffer.writeu8(bytes, offset + 1, buffer.readu8(bytes, offset + 2))
        buffer.writeu8(bytes, offset + 2, b)
        buffer.writeu8(bytes, offset + 3, a)
    end
    return bytes
end

-- Only SHA-pinned physical spans established by the native reader are accepted.
-- This performs crypto on those slices; it never reads a PAZ index or discovers names.
local function original_png(ctx, source, span, key, xor_byte)
    if span.offset < 0 or span.offset % 1 ~= 0 or span.aligned_size <= 0
        or span.aligned_size > LIMIT or span.aligned_size % 8 ~= 0
        or span.stored_size <= 0 or span.stored_size > span.aligned_size
        or span.unpacked_size <= 0 or span.unpacked_size > LIMIT
        or span.offset > source:size() or span.aligned_size > source:size() - span.offset then
        fail(ctx, "BOUNDS", "Perseus original texture slice is outside its pinned source")
    end
    local scheme = key.archive_keys.sys
    local bytes = source:read(span.offset, span.aligned_size)
    for offset = 0, buffer.len(bytes) - 1 do
        if offset % 4096 == 0 then ctx.check_cancelled() end
        buffer.writeu8(bytes, offset, bit32.bxor(buffer.readu8(bytes, offset), xor_byte))
    end
    bytes = swap_words(ctx, bytes)
    bytes = swap_words(ctx, ctx.crypto.blowfish(bytes, hex(ctx, scheme.data_key_hex), true))
    if not span.packed then
        local material = string.lower(span.name) .. string.format(" %08X ", span.unpacked_size)
            .. scheme.type_passwords.png
        local entry_key = ctx.encoding.encode(material, "cp932")
        local skip = key.rc4_skip_crc and bit32.band(bit32.rshift(ctx.hash.crc32(entry_key), 12), 255) or 0
        bytes = ctx.crypto.rc4(bytes, entry_key, skip)
    end
    local stored = buffer.create(span.stored_size)
    buffer.copy(stored, 0, bytes, 0, span.stored_size)
    if span.packed then
        bytes = ctx.codec.inflate(stored, span.unpacked_size)
    else
        if span.stored_size ~= span.unpacked_size then
            fail(ctx, "INTEGRITY", "Perseus original texture length disagrees with its native descriptor")
        end
        bytes = stored
    end
    if ctx.hash.sha256(bytes) ~= span.sha256 then
        fail(ctx, "INTEGRITY", "Perseus original texture identity disagrees with its native descriptor")
    end
    return bytes
end

function M.apply(ctx, edition)
    local molebox = ctx.load("common/molebox.luau")
    local copy = ctx.load("common/copy.luau")
    local root = if ctx.exists("patch", edition.loader) then "patch" else "source"
    local executable = pinned(ctx, root, edition.loader, edition.loader_sha256)
    local recovered = molebox.recover_loader(ctx, executable, edition.marker, edition.source_substitution)
    local parameters = molebox.loader_parameters(ctx, recovered, edition.label, edition.inner_substitution)
    local catalog = molebox.catalog(ctx, executable, parameters.seed, parameters.substitution)
    local package_seed = nil
    for _, item in catalog do
        if item.name == edition.package and item.flags == 0 then
            if package_seed then fail(ctx, "INTEGRITY", "Perseus catalog repeats its selected package") end
            package_seed = item.seed
        elseif item.name ~= "default" or item.flags ~= 1 then
            fail(ctx, "UNSUPPORTED_INPUT", "Perseus catalog contains an unaccounted package")
        end
    end
    if not package_seed then fail(ctx, "INTEGRITY", "Perseus catalog has no selected package") end
    local package_root = if ctx.exists("patch", edition.package) then "patch" else "source"
    local package = molebox.open(ctx, pinned(ctx, package_root, edition.package, edition.package_sha256),
        package_seed, parameters.substitution)
    if #package.nodes ~= #edition.members or package.block_count ~= edition.blocks then
        fail(ctx, "INTEGRITY", "Perseus package membership or block count disagrees with its edition")
    end
    local candidates = {}
    local expected_roles = {}
    for _, member in edition.members do
        if member.role ~= "musica_script" and member.role ~= "bmp_xor" and member.role ~= "bmp"
            and member.role ~= "font" and member.role ~= "ass" then
            fail(ctx, "UNSUPPORTED_INPUT", "Perseus package contains an unaccounted member role")
        end
        if string.find(member.path, "[^%w_.%-]") or member.path == "." or member.path == ".." then
            fail(ctx, "INTEGRITY", "Perseus member binding is not an original flat resource name")
        end
        table.insert(candidates, {path = member.path, member = member,
            uppercase_utf16le = ctx.encoding.encode(string.upper(member.path), "utf16le")})
        expected_roles[member.role] = (expected_roles[member.role] or 0) + 1
    end
    for role, count in edition.roles do
        if expected_roles[role] ~= count then fail(ctx, "INTEGRITY", "Perseus member role count disagrees with its edition") end
    end
    local bindings = molebox.bind_paths(ctx, package, candidates)
    local key_root = if ctx.exists("source", "key.toml") then "source" else "patch"
    local profile = ctx.open(key_root, "key.toml")
    if profile:size() == 0 or profile:size() > 1024 * 1024 then
        fail(ctx, "BOUNDS", "Perseus requires a bounded canonical private key.toml")
    end
    local key = ctx.toml.decode(ctx.encoding.decode(profile:read(0, profile:size()), "utf8"))
    if key.schema ~= "astra.emu.musica.keys.v2" or key.paz_version ~= 2
        or not key.archive_keys or not key.archive_keys.sys then
        fail(ctx, "UNSUPPORTED_INPUT", "Perseus requires the canonical native version-two profile")
    end
    local sys = pinned(ctx, "source", "sys.paz", edition.sys_sha256)
    local overrides = key.texture_overrides or {}
    local generated = {}
    local extracted = {}
    for index, node in package.nodes do
        ctx.check_cancelled()
        local member = bindings[node].member
        if node.size ~= member.size or node.size > LIMIT then
            fail(ctx, "BOUNDS", "Perseus member exceeds its exact bounded edition size")
        end
        local bytes = buffer.create(node.size)
        molebox.read_member(ctx, package, node, function(offset, block)
            buffer.copy(bytes, offset, block)
        end)
        if ctx.hash.sha256(bytes) ~= member.sha256 then
            fail(ctx, "INTEGRITY", "Perseus decoded member identity does not match its edition")
        end
        local output = prefix(member.role, member.path)
        if member.role == "bmp_xor" or member.role == "bmp" then
            if member.role == "bmp_xor" then
                local span = edition.originals[member.path]
                if not span or string.lower(span.name) ~= string.lower(member.path) then
                    fail(ctx, "INTEGRITY", "Perseus XOR member has no exact native original binding")
                end
                bytes = molebox.xor_bitmap_png(ctx, bytes, original_png(ctx, sys, span, key, edition.sys_xor_byte))
            else
                bytes = M.bitmap_png(ctx, bytes)
            end
            local role = if member.role == "bmp_xor" then "sys" else "bg"
            overrides["musica:/" .. role .. "/" .. member.path] = output
            generated[output] = true
        end
        write(ctx, output, bytes)
        extracted[member.path] = member.role
        -- Release block, bitmap and font intermediates within the fixed VM cap.
        collectgarbage("collect")
        ctx.progress("Restoring checked Perseus resources", index, #package.nodes)
    end
    local tracks = {}
    local font_paths = {}
    for _, font in edition.fonts do
        if extracted[font] ~= "font" then fail(ctx, "INTEGRITY", "Perseus subtitle font is not an extracted member") end
        table.insert(font_paths, prefix("font", font))
    end
    for _, track in edition.tracks do
        if extracted[track.ass] ~= "ass" then fail(ctx, "INTEGRITY", "Perseus subtitle track is not an extracted member") end
        table.insert(tracks, {clock = "movie", resource = track.resource,
            ass = prefix("ass", track.ass), fonts = font_paths})
    end
    local copied = {}
    for _, path in pairs(overrides) do
        if not generated[path] and not copied[path] then
            copy.files(ctx, key_root, {path})
            copied[path] = true
        end
    end
    key.texture_overrides = overrides
    copy.files(ctx, "source", edition.files)
    write(ctx, "key.toml", buffer.fromstring(ctx.toml.encode(key)))
    write(ctx, "localized/perseus/subtitles.json", buffer.fromstring(ctx.json.encode({version = 1, tracks = tracks})))
    return {plugin_id = "astra.emu.musica", configuration = {
        profile_file = "key.toml", script_encoding = "gbk", launch_mode = "title",
        script_directory = "localized/perseus/scripts",
        subtitle_map = "localized/perseus/subtitles.json",
    }}
end

return M
