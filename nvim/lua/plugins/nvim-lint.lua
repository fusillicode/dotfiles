return {
  'mfussenegger/nvim-lint',
  config = function()
    local lint = require('lint')

    lint.linters_by_ft = {
      dockerfile = { 'hadolint', },
      elixir = { 'credo', },
      markdown = { 'vale', },
      javascript = { 'eslint_d', },
      typescript = { 'eslint_d', },
      typescriptreact = { 'eslint_d', },
    }

    vim.api.nvim_create_autocmd('BufWritePost', {
      group = vim.api.nvim_create_augroup('FormatBufferWithNvimLint', { clear = true, }),
      callback = function() lint.try_lint() end,
    })
  end,
}
