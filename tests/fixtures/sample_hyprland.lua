-- Sample Hyprland Lua config used only by the Hyprland Mod test-suite.
-- It is never a real user's config.

require("colors")

hl.monitor({ output = "", mode = "preferred", position = "auto", scale = "auto" })

local terminal = "kitty"
hl.env("XCURSOR_SIZE", "24")

hl.config({
    general = {
        gaps_in = 5,
        gaps_out = 20,
        border_size = 2,
        col = {
            active_border = { colors = { "rgba(33ccffee)", "rgba(00ff99ee)" }, angle = 45 },
            inactive_border = "rgba(595959aa)",
        },
        layout = "dwindle",
    },
    decoration = {
        rounding = 10,
        shadow = { enabled = true, range = 4, color = 0xee1a1a1a },
        blur = { enabled = true, size = 3, passes = 1, vibrancy = 0.1696 },
    },
    animations = { enabled = true },
})

hl.curve("easeOutQuint", { type = "bezier", points = { {0.23, 1}, {0.32, 1} } })
hl.curve("linear",       { type = "bezier", points = { {0, 0},    {1, 1}    } })
hl.curve("easy",         { type = "spring", mass = 1, stiffness = 238.1191, dampening = 24.21 })
-- hl.curve("commented", { type = "bezier", points = { {0, 0}, {1, 1} } })
--[[
hl.curve("blockcommented", { type = "bezier", points = { {0, 0}, {1, 1} } })
]]

hl.animation({ leaf = "global",    enabled = true,  speed = 10,   bezier = "default" })
hl.animation({ leaf = "windows",   enabled = true,  speed = 4.79, spring = "easy" })
hl.animation({ leaf = "windowsIn", enabled = true,  speed = 4.1,  bezier = "easeOutQuint", style = "popin 87%" })
hl.animation({ leaf = "fade",      enabled = false })

hl.config({
    input = {
        kb_layout = "us",
        kb_options = "",
        touchpad = { natural_scroll = false },
    },
})

hl.bind("SUPER + Q", hl.dsp.exec_cmd(terminal))
