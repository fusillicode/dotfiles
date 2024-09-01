return {
  'gregorias/coerce.nvim',
  keys = { { '<leader>u', mode = { 'n', 'v', }, }, },
  config = function()
    require('coerce').setup({
      default_mode_keymap_prefixes = {
        normal_mode = '<leader>u',
        motion_mode = '<leader>u',
        visual_mode = '<leader>u',
      },
    })
  end,
}
