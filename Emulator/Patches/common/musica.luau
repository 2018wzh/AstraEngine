-- Only legacy DLL data extraction belongs here. PAZ, SC, encodings and fonts
-- remain native Musica consumers; unchanged PAZ files are never repacked.
local musica = {}
local roles = { "bg", "bgm", "scr", "st", "sys", "se", "voice", "mov" }

local function reject(ctx, message)
    ctx.fail("UNSUPPORTED_INPUT", message)
end

local function checked_source(ctx, root, path, fingerprint)
    local source = ctx.open(root, path)
    if source:size() ~= fingerprint.size or source:sha256() ~= fingerprint.sha256 then
        reject(ctx, "Musica input fingerprint does not match the supported edition")
    end
    return source
end

local function origin_hash(raw)
    local hash = 0
    for at = 0, buffer.len(raw) - 1 do
        local byte = buffer.readu8(raw, at)
        if byte ~= 0 then
            hash = (hash * 131 + byte) % 2147483648
        end
    end
    return hash
end

-- The unchanged second pointer is the native lookup key. The DLL replaces
-- only the first (display-name) pointer; hashing decoded Unicode loses identity.
function musica.names(ctx, executable, patch, layout)
    local pe = ctx.load("common/pe.luau")
    local original = pe.parse(ctx, executable)
    local translated = pe.parse(ctx, patch)
    if original.machine ~= 0x14c or translated.machine ~= 0x14c then
        reject(ctx, "Musica name table requires the supported PE32 inputs")
    end
    local names = {}
    for index, target_rva in ipairs(layout.targets) do
        ctx.check_cancelled()
        local pair = pe.offset(original, layout.table_rva + (index - 1) * 8, 8)
        local source_va = original.reader:u32le(pair + 4)
        if source_va < original.base then
            reject(ctx, "Musica name source pointer is outside the image")
        end
        local source_offset = pe.offset(original, source_va - original.base, 1)
        local raw = original.reader:cstring(source_offset, 4097)
        pe.offset(original, source_va - original.base, buffer.len(raw) + 1)
        if buffer.len(raw) == 0 or buffer.len(raw) > 4096 then
            reject(ctx, "Musica source name is empty or oversized")
        end
        -- Validate the actual script encoding, but hash original bytes.
        ctx.encoding.decode(raw, "gbk")
        local target_offset = pe.offset(translated, target_rva, 1)
        local target = translated.reader:cstring(target_offset, 4097)
        pe.offset(translated, target_rva, buffer.len(target) + 1)
        if buffer.len(target) == 0 or buffer.len(target) > 4096 then
            reject(ctx, "Musica translated name is empty or oversized")
        end
        local text = ctx.encoding.decode(target, "gbk")
        local hash = origin_hash(raw)
        table.insert(names, { source_hash = hash, source_sha256 = ctx.hash.sha256(raw), text = text })
    end
    return {
        version = 2,
        source_encoding = "gbk",
        hash_collision_policy = "first_record",
        names = names,
        dialogue = ctx.json.decode("[]"),
    }
end

function musica.apply_names(ctx, edition)
    -- r18_cn has a genuine rank-5 overlay consumer in the original executable.
    -- These inputs have no such directory; another edition needs native pairing.
    if edition.reject_r18 and ctx.exists("source", "r18_cn") then
        reject(ctx, "This edition does not include a verified r18_cn archive overlay")
    end
    local executable = checked_source(ctx, "source", edition.executable, edition.exe)
    local patch_root = if ctx.exists("patch", edition.dll) then "patch" else "source"
    local patch = checked_source(ctx, patch_root, edition.dll, edition.patch)
    local sources = {}
    for index, role in ipairs(roles) do
        ctx.check_cancelled()
        ctx.progress("Checking Musica resource identity", index - 1, #roles)
        sources[role] = checked_source(ctx, "source", role .. ".paz", edition.archives[role])
    end
    local names = musica.names(ctx, executable, patch, edition.layout)
    local key_root = if ctx.exists("source", "key.toml") then "source" else "patch"
    local key_source = ctx.open(key_root, "key.toml")
    if key_source:size() == 0 or key_source:size() > 1024 * 1024 then
        reject(ctx, "Musica requires an existing bounded private key.toml")
    end
    local key_text = ctx.encoding.decode(key_source:read(0, key_source:size()), "utf8")
    local key = ctx.toml.decode(key_text)
    -- Native Musica validates keys.v2 and actual archive/resource consumers.
    -- TOML is read here only to preserve explicitly referenced independent files.
    if key.schema ~= "astra.emu.musica.keys.v2" then
        reject(ctx, "Musica requires a canonical keys.v2 private key.toml")
    end
    local copy = ctx.load("common/copy.luau")
    for index, role in ipairs(roles) do
        ctx.check_cancelled()
        ctx.progress("Copying unchanged Musica resources", index - 1, #roles)
        ctx.copy("source", role .. ".paz", role .. ".paz")
    end
    ctx.copy(key_root, "key.toml", "key.toml")
    local copied = {}
    for _, path in pairs(key.texture_overrides or {}) do
        if not copied[path] then
            copy.files(ctx, key_root, { path })
            copied[path] = true
        end
    end
    local encoded = buffer.fromstring(ctx.json.encode(names))
    local output = ctx.create("musica-names.json")
    output:write(0, encoded)
    output:close()
    ctx.progress("Musica native localization data prepared", #roles, #roles)
    return {
        plugin_id = "astra.emu.musica",
        configuration = {
            profile_file = "key.toml",
            script_encoding = "gbk",
            launch_mode = "title",
            translation_table = "musica-names.json",
            translation_mode = "replace",
        },
    }
end

return musica
