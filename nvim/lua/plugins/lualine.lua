return {
  'nvim-lualine/lualine.nvim',
  event = 'VeryLazy',
  dependencies = {
    {
      "linrongbin16/lsp-progress.nvim",
      opts = {
        client_format = function(client_name, spinner, series_messages)
          local first_message = series_messages[1]
          return first_message ~= nil
              and "[" .. client_name .. "] " .. spinner .. " " .. first_message
              or nil
        end,
        format = function(client_messages)
          if #client_messages > 0 then
            return table.concat(client_messages, " ")
          end
          return ""
        end,
      },
    },
  },
  config = function()
    require("lualine").setup({
      options = {
        component_separators = '',
        icons_enabled = false,
        section_separators = '',
      },
      sections = {
        lualine_a = {},
        lualine_b = {},
        lualine_c = { { 'diagnostics', sources = { 'nvim_diagnostic' } }, { 'filename', file_status = true, path = 1 } },
        lualine_x = { { 'diagnostics', sources = { 'nvim_workspace_diagnostic' } }, { require("lsp-progress").progress } },
        lualine_y = {},
        lualine_z = {}
      },
    })
  end
}
