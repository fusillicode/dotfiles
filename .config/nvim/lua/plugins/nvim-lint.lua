return {
  'mfussenegger/nvim-lint',
  config = function()
    local lint = require('lint')

    lint.linters_by_ft = {
      dockerfile = { 'hadolint', },
      elixir = { 'credo', },
      javascript = { 'eslint_d', },
      sql = { 'sqruff', },
      typescript = { 'eslint_d', },
      typescriptreact = { 'eslint_d', },
    }

    local severities = {
      Error = vim.diagnostic.severity.ERROR,
      Warning = vim.diagnostic.severity.WARN,
    }

    lint.linters.sqruff.parser = function(output, _)
      if vim.trim(output) == '' or output == nil then
        return {}
      end

      local decoded = vim.json.decode(output)
      local diagnostics = {}
      local messages = decoded['<string>']

      for _, msg in ipairs(messages or {}) do
        table.insert(diagnostics, {
          lnum = msg.range.start.line - 1,
          end_lnum = msg.range['end'].line - 1,
          col = msg.range.start.character - 1,
          end_col = msg.range['end'].character - 1,
          message = msg.message,
          code = vim.NIL == msg.code and 'sqruff' or msg.code,
          source = msg.source,
          severity = assert(severities[msg.severity], 'missing mapping for severity ' .. msg.severity),
        })
      end

      return diagnostics
    end

    vim.api.nvim_create_autocmd('BufWritePost', {
      group = vim.api.nvim_create_augroup('FormatBufferWithNvimLint', { clear = true, }),
      callback = function() lint.try_lint() end,
    })
  end,
}
