local wezterm = require 'wezterm'

config = wezterm.config_builder()

config.animation_fps = 1
config.colors = {
  foreground = 'white',
  tab_bar = {
    active_tab = {
      bg_color = 'black',
      fg_color = 'white',
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
config.inactive_pane_hsb = { brightness = 0.3 }
config.hide_tab_bar_if_only_one_tab = true
config.line_height = 1.2
config.show_new_tab_button_in_tab_bar = false
config.switch_to_last_active_tab_when_closing_tab = true
config.text_blink_rate = 0
config.window_decorations = "RESIZE"
config.window_padding = { left = 0, right = 0, top = 0, bottom = 0 }
config.window_frame = {
  active_titlebar_bg = 'black',
  inactive_titlebar_bg = 'black',
}
 
return config