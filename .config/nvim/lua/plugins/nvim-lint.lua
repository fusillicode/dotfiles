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
      return require('nvrim').linters.sqruff.parser(output)
    end

    vim.api.nvim_create_autocmd('BufWritePost', {
      group = vim.api.nvim_create_augroup('FormatBufferWithNvimLint', { clear = true, }),
      callback = function() lint.try_lint() end,
    })
  end,
}
