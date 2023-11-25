return {
  'mfussenegger/nvim-lint',
  config = function()
    vim.api.nvim_create_autocmd({ 'BufWritePost', }, {
      group = vim.api.nvim_create_augroup('FormatBufferWithNvimLint', { clear = true, }),
      callback = function()
        require('lint').try_lint()
      end,
    })
  end,
}
