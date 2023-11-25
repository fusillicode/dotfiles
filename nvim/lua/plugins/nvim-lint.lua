return {
  'mfussenegger/nvim-lint',
  config = function()
    require('lint').linters_by_ft = {
      dockerfile = { 'hadolint', },
      markdownlint = { 'markdown', },
    }
    vim.api.nvim_create_autocmd({ 'BufWritePost', }, {
      group = vim.api.nvim_create_augroup('FormatBufferWithNvimLint', { clear = true, }),
      callback = function()
        require('lint').try_lint()
      end,
    })
  end,
}
