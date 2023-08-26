local wezterm = require 'wezterm'

local config = wezterm.config_builder()

config.animation_fps = 1
config.colors = {
  foreground = 'white',
  tab_bar = {
    active_tab = {
      bg_color = 'black',
      fg_color = 'lime',
    },
    inactive_tab = {
      bg_color = 'black',
      fg_color = 'grey',
    }
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
  { key = 'LeftArrow',  mods = 'OPT',       action = wezterm.action.SendKey { key = 'b', mods = 'ALT', }, },
  { key = 'RightArrow', mods = 'OPT',       action = wezterm.action.SendKey { key = 'f', mods = 'ALT' }, },
  { key = 'h',          mods = 'CMD|SHIFT', action = wezterm.action.ActivatePaneDirection 'Left', },
  { key = 'l',          mods = 'CMD|SHIFT', action = wezterm.action.ActivatePaneDirection 'Right', },
  { key = 'k',          mods = 'CMD|SHIFT', action = wezterm.action.ActivatePaneDirection 'Up', },
  { key = 'j',          mods = 'CMD|SHIFT', action = wezterm.action.ActivatePaneDirection 'Down', },
  {
    key = 'n', mods = 'CMD|SHIFT', action = wezterm.action.SplitVertical { domain = "CurrentPaneDomain" }
  },
  {
    key = 't', mods = 'CMD|SHIFT', action = wezterm.action.SplitPane { direction = "Right", size = { Percent = 59 } }
  },
  { key = 'p',     mods = 'CMD|SHIFT', action = wezterm.action.ActivateCommandPalette, },
  { key = 'x',     mods = 'CMD',       action = wezterm.action.ActivateCopyMode, },
  { key = 'a',     mods = 'CMD|SHIFT', action = wezterm.action.TogglePaneZoomState, },
  { key = '[',     mods = 'CTRL|OPT',  action = wezterm.action.MoveTabRelative(-1), },
  { key = ']',     mods = 'CTRL|OPT',  action = wezterm.action.MoveTabRelative(1), },
  { key = 'Enter', mods = 'ALT',       action = wezterm.action.Nop, },
}
local copy_mode = nil
if wezterm.gui then
  copy_mode = wezterm.gui.default_key_tables().copy_mode
  table.insert(
    copy_mode,
    {
      key = 'x',
      action = wezterm.action.CopyMode { SetSelectionMode = 'Line' },
    }
  )
end
config.key_tables = { copy_mode = copy_mode }
config.line_height = 1.2
config.show_new_tab_button_in_tab_bar = false
config.switch_to_last_active_tab_when_closing_tab = true
config.text_blink_rate = 0
config.window_decorations = "RESIZE"
config.window_padding = { left = 0, right = 0, top = 0, bottom = 0 }
config.window_frame = { active_titlebar_bg = 'black', inactive_titlebar_bg = 'black' }

return config
