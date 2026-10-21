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
local events = {}

-- Strict hl.dsp: only the dispatchers listed in Hyprland 0.56's hl.meta.lua stubs exist.
local DSP = {
    [""] = { "dpms", "event", "exec_cmd", "exec_raw", "exit", "focus", "force_idle",
             "force_renderer_reload", "global", "layout", "no_op", "pass", "release_input_capture",
             "send_key_state", "send_shortcut", "submap" },
    cursor = { "move", "move_to_corner" },
    group = { "active", "lock", "lock_active", "move_window", "next", "prev", "toggle" },
    window = { "alter_zorder", "bring_to_top", "center", "clear_tags", "close", "cycle_next",
               "deny_from_group", "drag", "float", "fullscreen", "fullscreen_state", "kill", "move",
               "pin", "pseudo", "resize", "set_prop", "signal", "swap", "tag", "toggle_swallow" },
    workspace = { "change_id", "move", "rename", "swap_monitors", "toggle_special" },
}
-- Table keys the binary accepts for some dispatchers (from its error messages).
local ALLOWED = {
    ["window.move"] = { direction = 1, x = 1, y = 1, relative = 1, workspace = 1, into_group = 1, out_of_group = 1 },
    ["window.swap"] = { direction = 1, target = 1, with = 1, other = 1, next = 1, prev = 1 },
    ["focus"] = { direction = 1, monitor = 1, window = 1, urgent_or_last = 1, last = 1, workspace = 1 },
    ["window.resize"] = { x = 1, y = 1, relative = 1, window = 1, keep_aspect_ratio = 1 },
    ["workspace.move"] = { monitor = 1, workspace = 1 },
}
local function show(v)
    if type(v) ~= "table" then return tostring(v) end
    local keys = {}
    for k in pairs(v) do keys[#keys + 1] = k end
    table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
    local parts = {}
    for _, k in ipairs(keys) do parts[#parts + 1] = tostring(k) .. "=" .. show(v[k]) end
    return "{" .. table.concat(parts, ",") .. "}"
end
local function make_ns(prefix, names)
    local ns = {}
    for _, n in ipairs(names) do
        local full = prefix == "" and n or (prefix .. "." .. n)
        ns[n] = function(arg, ...)
            assert(select("#", ...) == 0, full .. ": too many arguments")
            local allowed = ALLOWED[full]
            if allowed and arg ~= nil then
                assert(type(arg) == "table", full .. ": expected a table")
                for k in pairs(arg) do assert(allowed[k], full .. ": unknown argument " .. tostring(k)) end
            end
            return { dispatcher = full, text = full .. "(" .. (arg == nil and "" or show(arg)) .. ")" }
        end
    end
    return setmetatable(ns, { __index = function(_, k) error("hl.dsp: no dispatcher " .. prefix .. "." .. tostring(k)) end })
end
local dsp = make_ns("", DSP[""])
for sub, names in pairs(DSP) do
    if sub ~= "" then rawset(dsp, sub, make_ns(sub, names)) end
end

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
    bind = function(keys, dispatcher, opts)
        assert(type(keys) == "string", "hl.bind: keys must be a string")
        assert(type(dispatcher) == "function" or (type(dispatcher) == "table" and dispatcher.dispatcher),
            "hl.bind: expected a dispatcher")
        if opts ~= nil then assert(type(opts) == "table", "hl.bind: opts must be a table") end
        events[#events + 1] = "bind " .. keys .. " -> "
            .. (type(dispatcher) == "table" and dispatcher.text or "<function>")
            .. (opts and (" " .. show(opts)) or "")
        return { set_enabled = noop }
    end,
    window_rule = function(spec)
        assert(type(spec) == "table" and type(spec.match) == "table", "hl.window_rule: needs a match table")
        events[#events + 1] = "rule " .. show(spec)
        return { set_enabled = noop }
    end,
    monitor = function(spec)
        assert(type(spec) == "table" and type(spec.output) == "string", "hl.monitor: needs an output")
        events[#events + 1] = "monitor " .. show(spec)
    end,
    on = function(event, cb)
        assert(type(event) == "string" and type(cb) == "function", "hl.on: expected (event, function)")
        events[#events + 1] = "on " .. event
        cb() -- run the handler now so its body is checked too
        return { remove = noop, is_active = noop }
    end,
    exec_cmd = function(cmd, rules)
        assert(type(cmd) == "string", "hl.exec_cmd: expected a command string")
        events[#events + 1] = "exec " .. cmd
    end,
    dsp = dsp,
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
for _, e in ipairs(events) do print(e) end
flat("", merged)
for _, c in ipairs(curves) do print("curve " .. c) end
for _, a in ipairs(anims) do print("anim " .. a) end
