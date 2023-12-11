return {
  'linrongbin16/lsp-progress.nvim',
  event = 'LspAttach',
  config = function()
    require('lsp-progress').setup({
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
    })
  end,
}
