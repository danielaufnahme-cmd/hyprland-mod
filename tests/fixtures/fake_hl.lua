-- Test harness: runs a Hyprland Lua config against a fake `hl` table and prints
-- what it configured. Usage: lua fake_hl.lua <config.lua>
local cfg_path = arg[1]
local dir = cfg_path:match("^(.*)/[^/]*$") or "."
package.path = dir .. "/?.lua;" .. package.path

local merged, curves, anims = {}, {}, {}
local function merge(dst, src)
    for k, v in pairs(src) do
        if type(v) == "table" and type(dst[k]) == "table" and v.colors == nil then
            merge(dst[k], v)
        else
            dst[k] = v
        end
    end
end

local noop = function() return "noop" end
hl = setmetatable({
    config = function(t) assert(type(t) == "table", "hl.config needs a table"); merge(merged, t) end,
    curve = function(name, spec)
        assert(type(name) == "string" and type(spec) == "table", "bad hl.curve")
        local p = spec.points or {}
        curves[#curves + 1] = name .. ":" .. tostring(spec.type)
            .. (p[1] and (":" .. p[1][1] .. "," .. p[1][2] .. "," .. p[2][1] .. "," .. p[2][2]) or "")
    end,
    animation = function(spec)
        assert(type(spec) == "table" and type(spec.leaf) == "string", "bad hl.animation")
        anims[#anims + 1] = table.concat({ spec.leaf, tostring(spec.enabled), tostring(spec.speed),
            tostring(spec.bezier or (spec.spring and ("spring:" .. spec.spring))), tostring(spec.style) }, "|")
    end,
    dsp = setmetatable({}, { __index = function() return noop end }),
}, { __index = function() return noop end })

dofile(cfg_path)

local function flat(prefix, t)
    local keys = {}
    for k in pairs(t) do keys[#keys + 1] = k end
    table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
    for _, k in ipairs(keys) do
        local v = t[k]
        local name = prefix == "" and tostring(k) or (prefix .. "." .. tostring(k))
        if type(v) == "table" then flat(name, v) else print("cfg " .. name .. "=" .. tostring(v)) end
    end
end
flat("", merged)
for _, c in ipairs(curves) do print("curve " .. c) end
for _, a in ipairs(anims) do print("anim " .. a) end
