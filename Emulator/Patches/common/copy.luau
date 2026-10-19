local copy = {}

local function parent(ctx, path)
    local directory = string.match(path, "^(.*)/[^/]+$")
    if directory and directory ~= "" then ctx.mkdir(directory) end
end

-- The caller supplies every file; no launcher/save/archive-priority guesses.
-- Items are a relative string, or {from="...", to="..."}.
function copy.files(ctx, root, paths)
    for i, item in ipairs(paths) do
        ctx.check_cancelled()
        local from = type(item) == "table" and item.from or item
        local to = type(item) == "table" and item.to or item
        parent(ctx, to)
        ctx.copy(root, from, to)
        ctx.progress("复制资源", i, #paths)
    end
end

-- keep(source_relative, kind, size) is mandatory: only the recipe decides
-- which resources are consumed. Links require an explicit ctx.copy call.
function copy.tree(ctx, root, source, output, keep)
    assert(type(keep) == "function", "copy.tree requires an explicit resource policy")
    local pending = {{source=source, output=output}}
    local completed = 0
    while #pending > 0 do
        ctx.check_cancelled()
        local current = table.remove(pending)
        if current.output ~= "" then ctx.mkdir(current.output) end
        for _, entry in ipairs(ctx.list(root, current.source)) do
            local from = current.source == "" and entry.name or current.source .. "/" .. entry.name
            local to = current.output == "" and entry.name or current.output .. "/" .. entry.name
            if keep(from, entry.kind, entry.size) then
                if entry.kind == "directory" then
                    table.insert(pending, {source=from, output=to})
                elseif entry.kind == "file" then
                    ctx.copy(root, from, to)
                    completed += 1
                    ctx.progress("复制资源", completed, nil)
                else
                    ctx.fail("UNSUPPORTED_INPUT", "required source link needs an explicit bounded copy")
                end
            end
        end
    end
end

return copy
