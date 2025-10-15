return {
  'mfussenegger/nvim-lint',
  config = function()
    local lint = require('lint')
    local nvrim = require('nvrim')

    lint.linters_by_ft = {
      dockerfile = { 'hadolint', },
      elixir = { 'credo', },
      javascript = { 'eslint_d', },
      sql = { 'sqruff', },
      typescript = { 'eslint_d', },
      typescriptreact = { 'eslint_d', },
    }

    lint.linters.sqruff.parser = function(output, _)
      return nvrim.linters.sqruff.parser(output)
    end

    vim.api.nvim_create_autocmd('BufWritePost', {
      group = vim.api.nvim_create_augroup('FormatBufferWithNvimLint', { clear = true, }),
      callback = function() lint.try_lint() end,
    })
  end,
}
