-- Necessary legacy ACR/FUYINPAK normalization only. PAZ/SC readers stay native.
local M = {}
local roles = {"bg", "bgm", "scr", "st", "sys", "se", "voice", "mov"}

local function fail(ctx, message)
    ctx.fail("INTEGRITY", message)
end

local function parent(ctx, path)
    local directory = string.match(path, "^(.*)/[^/]+$")
    if directory then ctx.mkdir(directory) end
end

local function write(ctx, path, bytes)
    parent(ctx, path)
    local output = ctx.create(path)
    output:write(0, bytes)
    output:close()
end

local function relative_image(ctx, path)
    if type(path) ~= "string" or path == "" or string.find(path, "[%z\\:]")
        or string.sub(path, 1, 1) == "/" or string.find(path, "//", 1, true) then
        fail(ctx, "Texture override must name an independent relative image")
    end
    local lower = string.lower(path)
    if string.sub(lower, -4) ~= ".png" then
        fail(ctx, "Native texture override input must be a standard PNG")
    end
    for component in string.gmatch(path, "[^/]+") do
        if component == "." or component == ".." then
            fail(ctx, "Texture override contains a traversal component")
        end
    end
    return path
end

function M.apply(ctx, edition)
    local legacy = ctx.load("common/musica_acr.luau")
    local copy = ctx.load("common/copy.luau")
    local inputs, audit, present = {}, {}, {}
    for _, item in edition.inputs do
        ctx.check_cancelled()
        local root = if item.patch and ctx.exists("patch", item.path) then "patch" else "source"
        local source = ctx.open(root, item.path)
        if source:size() ~= item.size or source:sha256() ~= item.sha256 then
            ctx.fail("UNSUPPORTED_INPUT", "Yorino input differs from its supported fingerprint: " .. item.path)
        end
        inputs[item.path] = {source=source, root=root}
        table.insert(audit, {path=item.path, size=item.size, sha256=item.sha256})
        if item.role then
            if present[item.role] then fail(ctx, "Duplicate required Musica archive role") end
            present[item.role] = true
        end
    end
    local key_source = ctx.open("source", "key.toml")
    if key_source:size() == 0 or key_source:size() > 1024 * 1024 then
        fail(ctx, "A bounded canonical private key.toml is required")
    end
    local key = ctx.toml.decode(ctx.encoding.decode(key_source:read(0, key_source:size()), "utf8"))
    if key.schema ~= "astra.emu.musica.keys.v2" or type(key.archive_keys) ~= "table" then
        fail(ctx, "Yorino requires a canonical Musica keys.v2 profile")
    end
    for _, role in roles do
        if not present[role] or type(key.archive_keys[role]) ~= "table" then
            fail(ctx, "Yorino requires all eight independent native archive roles")
        end
    end
    table.insert(audit, {path="key.toml", size=key_source:size(), sha256=key_source:sha256()})
    local names = legacy.read(ctx, inputs["name.acr"].source)
    local dialogue = legacy.read(ctx, inputs["AsyncScript.acr"].source)
    if #names ~= edition.names or #dialogue ~= edition.dialogue then
        fail(ctx, "Yorino ACR ordered record counts disagree with the supported edition")
    end
    local translations, record_audit = legacy.translation_table(ctx, names, dialogue,
        {source="gbk", names="gbk", dialogue="cp932"})
    local backgrounds = legacy.cnbg_index(ctx, inputs["cnbg.paz"].source)
    if #backgrounds ~= #edition.background_crc32 then
        fail(ctx, "Yorino cnbg resource count disagrees with the supported edition")
    end
    for _, item in edition.inputs do
        if not item.patch then copy.files(ctx, "source", {item.path}) end
    end
    local overrides = key.texture_overrides or {}
    if type(overrides) ~= "table" then fail(ctx, "Texture overrides must be a resource-to-path map") end
    local copied = {}
    for resource, path in overrides do
        if type(resource) ~= "string" then fail(ctx, "Texture override identity must be a string") end
        relative_image(ctx, path)
        if not copied[path] then
            local source = ctx.open("source", path)
            table.insert(audit, {path=path, size=source:size(), sha256=source:sha256()})
            copy.files(ctx, "source", {path})
            copied[path] = true
        end
    end
    local normalized = {}
    for index, entry in backgrounds do
        ctx.check_cancelled()
        local path = "yorino-backgrounds/" .. entry.resource
        relative_image(ctx, path)
        if copied[path] then fail(ctx, "Generated background path conflicts with an existing override input") end
        local png = legacy.cnbg_png(ctx, inputs["cnbg.paz"].source, entry,
            {width=edition.width, height=edition.height, crc32=edition.background_crc32[index]})
        write(ctx, path, png)
        local uri = "musica:/bg/" .. entry.resource
        overrides[uri] = path
        table.insert(normalized, {resource=uri, path=path, sha256=ctx.hash.sha256(png),
            source_crc32=edition.background_crc32[index], width=edition.width, height=edition.height})
        -- Full-size bitmap intermediates must retire before the next image.
        collectgarbage("collect")
        ctx.progress("Yorino standard background images", index, #backgrounds)
    end
    key.texture_overrides = overrides
    -- Only real root-level font files are selected; no invented font-family/path.
    local fonts = {}
    for _, item in ctx.list("source", "") do
        if item.kind == "file" then
            local suffix = string.lower(string.sub(item.name, -4))
            if suffix == ".ttf" or suffix == ".otf" or suffix == ".ttc" then
                table.insert(fonts, item.name)
            end
        end
    end
    table.sort(fonts)
    for _, path in fonts do
        local source = ctx.open("source", path)
        table.insert(audit, {path=path, size=source:size(), sha256=source:sha256()})
        copy.files(ctx, "source", {path})
    end
    write(ctx, "key.toml", buffer.fromstring(ctx.toml.encode(key)))
    write(ctx, "yorino-translations.json", buffer.fromstring(ctx.json.encode(translations)))
    -- Raw ACR identity is complete. Native endpoint coverage is a separate gate:
    -- 27 names occur as Message speakers, two only in original EXE raw data,
    -- and the fifteen zero-based rows below still have no located use-site.
    write(ctx, "yorino-coverage.json", buffer.fromstring(ctx.json.encode({
        version=1, status="native_endpoint_coverage_incomplete", converted_verified=false,
        acr_source_identity="all_ordered_rows_raw_index_hash_verified",
        names=#names, dialogue=#dialogue, record_audit=record_audit,
        native_endpoint_coverage=edition.coverage,
        dialogue_target_semantics="literal_cp932_overlay_no_vm_markup",
        alpha_policy="preserve_rgba_including_zero_alpha_native_base_background_opaque_composition",
        inputs=audit, backgrounds=normalized,
    })))
    local configuration = {profile_file="key.toml", script_encoding="gbk", launch_mode="title",
        scr_archive="cnscr.paz", sys_archive="cnsys.paz",
        translation_table="yorino-translations.json", translation_mode="overlay"}
    if #fonts > 0 then configuration.font_path = fonts[1] end
    return {plugin_id="astra.emu.musica", configuration=configuration}
end

return M
