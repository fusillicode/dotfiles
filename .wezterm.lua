local wezterm = require 'wezterm'
local act = wezterm.action
local config = wezterm.config_builder()

-- Equivalent to POSIX basename(3)
-- Given '/foo/bar' returns 'bar'
-- Given 'c:\\foo\\bar' returns 'bar'
local function base_name(s)
  return string.gsub(s, '(.*[/\\])(.*)', '%2')
end

local function up_and_down_in_editor(key)
  return function(win, pane)
    local process_name = base_name(pane:get_foreground_process_name())
    if 'hx' == process_name or 'nvim' == process_name then
      win:perform_action(act.SendKey { key = key, mods = 'CTRL', }, pane)
    else
      win:perform_action(act.ActivateCopyMode, pane)
    end
  end
end

config.animation_fps = 1

local background = '#14161b'
local foreground = 'white'
config.colors = {
  background = background,
  foreground = foreground,
  cursor_bg = foreground,
  cursor_border = background,
  tab_bar = {
    active_tab = { bg_color = background, fg_color = foreground, },
    inactive_tab = { bg_color = background, fg_color = 'grey', },
  },
}
config.font_size = 17
-- No ligatures thanks
config.harfbuzz_features = { 'calt=0', 'clig=0', 'liga=0', }
config.line_height = 1.1

config.keys = {
  { key = 'LeftArrow',  mods = 'OPT',         action = act.SendKey { key = 'b', mods = 'ALT', }, },
  { key = 'RightArrow', mods = 'OPT',         action = act.SendKey { key = 'f', mods = 'ALT', }, },
  { key = 'h',          mods = 'SUPER|SHIFT', action = act.ActivatePaneDirection 'Left', },
  { key = 'l',          mods = 'SUPER|SHIFT', action = act.ActivatePaneDirection 'Right', },
  { key = 'k',          mods = 'SUPER|SHIFT', action = act.ActivatePaneDirection 'Up', },
  { key = 'j',          mods = 'SUPER|SHIFT', action = act.ActivatePaneDirection 'Down', },
  { key = 'n',          mods = 'SUPER|SHIFT', action = act.SplitVertical { domain = 'CurrentPaneDomain', }, },
  { key = 't',          mods = 'SUPER|SHIFT', action = act.SplitPane { direction = 'Right', size = { Percent = 60, }, }, },
  { key = 't',          mods = 'SUPER',       action = act.EmitEvent 'open-tab-with-custom-layout', },
  { key = 'p',          mods = 'SUPER',       action = act.ActivateCommandPalette, },
  { key = 'x',          mods = 'SUPER',       action = act.ActivateCopyMode, },
  { key = 'f',          mods = 'SUPER',       action = act.Search 'CurrentSelectionOrEmptyString', },
  { key = 'a',          mods = 'SUPER|SHIFT', action = act.TogglePaneZoomState, },
  { key = '[',          mods = 'CTRL|OPT',    action = act.MoveTabRelative(-1), },
  { key = ']',          mods = 'CTRL|OPT',    action = act.MoveTabRelative(1), },
  { key = 'Enter',      mods = 'ALT',         action = act.Nop, },
  { key = 'd',          mods = 'CTRL',        action = wezterm.action_callback(up_and_down_in_editor('d')), },
  { key = 'u',          mods = 'CTRL',        action = wezterm.action_callback(up_and_down_in_editor('u')), },
}

local copy_mode = wezterm.gui.default_key_tables().copy_mode
for _, keymap in pairs({
  { key = '/', action = act.CopyMode 'EditPattern', },
  { key = 'x', action = act.CopyMode { SetSelectionMode = 'Line', }, },
  { key = 'd', mods = 'CTRL',                                        action = act.CopyMode { MoveByPage = 0.5, }, },
  { key = 'u', mods = 'CTRL',                                        action = act.CopyMode { MoveByPage = -0.5, }, },
  { key = 'y', mods = 'NONE', action = act.Multiple { { CopyTo = 'ClipboardAndPrimarySelection', }, },
  },
}) do table.insert(copy_mode, keymap) end

local search_mode = wezterm.gui.default_key_tables().search_mode
for _, keymap in pairs({
  { key = 'Enter', mods = 'NONE',  action = wezterm.action.CopyMode 'NextMatch', },
  { key = 'Enter', mods = 'SHIFT', action = wezterm.action.CopyMode 'PriorMatch', },
  { key = 'c',     mods = 'CTRL',  action = wezterm.action.CopyMode 'ClearPattern', },
}) do table.insert(search_mode, keymap) end

config.key_tables = { copy_mode = copy_mode, search_mode = search_mode, }

config.hide_tab_bar_if_only_one_tab = true
config.show_close_tab_button_in_tabs = false
config.show_new_tab_button_in_tab_bar = false
config.switch_to_last_active_tab_when_closing_tab = true
config.text_blink_rate = 0
config.window_decorations = 'RESIZE'
config.window_padding = { left = 0, right = 0, top = 0, bottom = 0, }
config.window_frame = { active_titlebar_bg = background, inactive_titlebar_bg = background, }

local split_perc = 0.69

-- 🥲 https://github.com/wez/wezterm/issues/3173
wezterm.on('window-config-reloaded', function(window, _)
  -- Approximately identify this gui window, by using the associated mux id
  local id = tostring(window:window_id())

  -- Maintain a mapping of windows that we have previously seen before in this event handler
  local seen = wezterm.GLOBAL.seen_windows or {}
  -- Set a flag if we haven't seen this window before
  local is_new_window = not seen[id]
  -- And update the mapping
  seen[id] = true
  wezterm.GLOBAL.seen_windows = seen

  -- Now act upon the flag
  if is_new_window then
    window:maximize()
    local active_pane = window:active_pane()
    active_pane:split { size = split_perc, }
  end
end)

wezterm.on('open-tab-with-custom-layout', function(window, _)
  local _, pane, _ = window:mux_window():spawn_tab({})
  pane:split { size = split_perc, }
end)

config.hyperlink_rules = wezterm.default_hyperlink_rules()

table.insert(config.hyperlink_rules, {
  regex = [[(~|/)?[^:\s]+(\:\d+){0,2}]],
  format = '$0',
})

wezterm.on('open-uri', function(_, pane, uri)
  -- It would be beautiful to just load the env and PATH and avoid
  -- pointing to the local bins
  local cmd = {
    os.getenv('HOME') .. '/.local/bin/oe',
    'nv',
    uri,
    pane:pane_id(),
  }

  local success, _, err = wezterm.run_child_process(cmd)
  if success then
    -- Prevent the default action from triggering
    return false
  end

  if err then
    -- Logging like this because I don't know how to print
    -- a table in lua without building a custom helper or rely
    -- on an external library...
    wezterm.log_error('Cmd')
    wezterm.log_error(cmd)
    wezterm.log_error('Error')
    wezterm.log_error(err)
  end
end)

return config
