return {
  'stevearc/conform.nvim',
  event = 'BufWritePre',
  cmd = { 'ConformInfo', 'ConformAt', },
  config = function()
    local conform = require('conform')

    conform.setup({
      formatters_by_ft = {
        css = { { 'prettierd', }, },
        graphql = { { 'prettierd', }, },
        html = { { 'prettierd', }, },
        javascript = { { 'prettierd', }, },
        markdown = { { 'prettierd', }, },
        sql = { { 'sqlfluff', }, },
        typescript = { { 'prettierd', }, },
        typescriptreact = { { 'prettierd', }, },
        yaml = { { 'prettierd', }, },
        ['*'] = { 'trim_whitespaces', 'trim_newlines', },
      },
      format_on_save = { timeout_ms = 500, lsp_fallback = true, },
    })

    vim.api.nvim_create_user_command('ConformAt', function(args)
      local range = nil
      if args.count ~= -1 then
        local end_line = vim.api.nvim_buf_get_lines(0, args.line2 - 1, args.line2, true)[1]
        range = {
          start = { args.line1, 0, },
          ['end'] = { args.line2, end_line:len(), },
        }
      end
      conform.format({ async = true, lsp_fallback = true, range = range, })
    end, { range = true, })
  end,
}
