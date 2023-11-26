return {
  'nvim-lualine/lualine.nvim',
  event = 'VeryLazy',
  dependencies = {
    {
      'linrongbin16/lsp-progress.nvim',
      opts = {
        spinner = { '', },
        client_format = function(client_name, _, series_messages)
          local first_message = series_messages[1]
          return first_message ~= nil
              and '[' .. client_name .. '] ' .. first_message
              or nil
        end,
        format = function(client_messages)
          return #client_messages > 0 and
              table.concat(client_messages, ' ')
              or ''
        end,
      },
    },
  },
  config = function()
    require('lualine').setup({
      options = {
        component_separators = '',
        icons_enabled = false,
        section_separators = '',
      },
      sections = {
        lualine_a = {},
        lualine_b = {},
        lualine_c = {
          {
            'diagnostics', sources = { 'nvim_diagnostic', },
          },
          { 'filename', file_status = true, path = 1, },
        },
        lualine_x = {
          { require('lsp-progress').progress, },
          {
            'diagnostics', sources = { 'nvim_workspace_diagnostic', },
          },
        },
        lualine_y = {},
        lualine_z = {},
      },
    })
  end,
}
