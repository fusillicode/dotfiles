local wezterm = require 'wezterm'
local act = wezterm.action
local config = wezterm.config_builder()

-- Equivalent to POSIX basename(3)
-- Given "/foo/bar" returns "bar"
-- Given "c:\\foo\\bar" returns "bar"
local function base_name(s)
  return string.gsub(s, '(.*[/\\])(.*)', '%2')
end

local function up_and_down_with_hx(key)
  return function(win, pane)
    if 'hx' == base_name(pane:get_foreground_process_name()) then
      win:perform_action(act.SendKey { key = key, mods = 'CTRL' }, pane)
    else
      win:perform_action(act.ActivateCopyMode, pane)
    end
  end
end

local background = '0f1419'
config.animation_fps = 1
config.colors = {
  foreground = 'white',
  background = background,
  tab_bar = {
    active_tab = { bg_color = background, fg_color = 'lime', },
    inactive_tab = { bg_color = background, fg_color = 'grey', }
  }
}
config.cursor_blink_ease_in = 'Constant'
config.cursor_blink_ease_out = 'Constant'
config.cursor_blink_rate = 0
config.font = wezterm.font('Monaco')
config.font_size = 16
config.inactive_pane_hsb = { brightness = 0.5 }
config.hide_tab_bar_if_only_one_tab = true

config.keys = {
  { key = 'LeftArrow',  mods = 'OPT',       action = act.SendKey { key = 'b', mods = 'ALT', }, },
  { key = 'RightArrow', mods = 'OPT',       action = act.SendKey { key = 'f', mods = 'ALT' }, },
  { key = 'h',          mods = 'CMD|SHIFT', action = act.ActivatePaneDirection 'Left', },
  { key = 'l',          mods = 'CMD|SHIFT', action = act.ActivatePaneDirection 'Right', },
  { key = 'k',          mods = 'CMD|SHIFT', action = act.ActivatePaneDirection 'Up', },
  { key = 'j',          mods = 'CMD|SHIFT', action = act.ActivatePaneDirection 'Down', },
  { key = 'n',          mods = 'CMD|SHIFT', action = act.SplitVertical { domain = "CurrentPaneDomain" } },
  { key = 't',          mods = 'CMD|SHIFT', action = act.SplitPane { direction = "Right", size = { Percent = 59 } } },
  { key = 'p',          mods = 'CMD',       action = act.ActivateCommandPalette, },
  { key = 'x',          mods = 'CMD',       action = act.ActivateCopyMode, },
  { key = 'a',          mods = 'CMD|SHIFT', action = act.TogglePaneZoomState, },
  { key = '[',          mods = 'CTRL|OPT',  action = act.MoveTabRelative(-1), },
  { key = ']',          mods = 'CTRL|OPT',  action = act.MoveTabRelative(1), },
  { key = 'Enter',      mods = 'ALT',       action = act.Nop, },
  { key = 'd',          mods = 'CTRL',      action = wezterm.action_callback(up_and_down_with_hx('d')), },
  { key = 'u',          mods = 'CTRL',      action = wezterm.action_callback(up_and_down_with_hx('u')), },
}

local copy_mode = nil
if wezterm.gui then
  copy_mode = wezterm.gui.default_key_tables().copy_mode
  for _, custom_copy_key in pairs({
    { key = '/', action = act.CopyMode 'EditPattern' },
    { key = 'x', action = act.CopyMode { SetSelectionMode = 'Line' }, },
    { key = 'd', mods = 'CTRL',                                       action = act.CopyMode { MoveByPage = 0.5 }, },
    { key = 'u', mods = 'CTRL',                                       action = act.CopyMode { MoveByPage = -0.5 } },
  }) do
    table.insert(copy_mode, custom_copy_key)
  end
end
config.key_tables = { copy_mode = copy_mode }

config.line_height = 1.2
config.show_new_tab_button_in_tab_bar = false
config.switch_to_last_active_tab_when_closing_tab = true
config.text_blink_rate = 0
config.window_decorations = "RESIZE"
config.window_padding = { left = 0, right = 0, top = 0, bottom = 0 }
config.window_frame = { active_titlebar_bg = background, inactive_titlebar_bg = background }

return config
