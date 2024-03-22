return {
  'mfussenegger/nvim-lint',
  config = function()
    local lint = require('lint')

    lint.linters_by_ft = {
      dockerfile = { 'hadolint', },
      elixir = { 'credo', },
      javascript = { 'eslint_d', },
      markdown = { 'vale', },
      sql = { 'sqlfluff', },
      typescript = { 'eslint_d', },
      typescriptreact = { 'eslint_d', },
    }

    vim.api.nvim_create_autocmd('BufWritePost', {
      group = vim.api.nvim_create_augroup('FormatBufferWithNvimLint', { clear = true, }),
      callback = function() lint.try_lint() end,
    })
  end,
}
