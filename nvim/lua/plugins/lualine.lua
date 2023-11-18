return {
  'nvim-lualine/lualine.nvim',
  event = 'VeryLazy',
  opts = {
    options = {
      component_separators = '',
      icons_enabled = false,
      section_separators = '',
      theme = 'auto',
    },
    sections = {
      lualine_a = {},
      lualine_b = {},
      lualine_c = { { 'diagnostics', sources = { 'nvim_diagnostic' } }, { 'filename', file_status = true, path = 1 } },
      lualine_x = { { 'diagnostics', sources = { 'nvim_workspace_diagnostic' } } },
      lualine_y = {},
      lualine_z = {}
    },
  }
}
