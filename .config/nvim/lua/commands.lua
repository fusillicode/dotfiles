vim.api.nvim_create_user_command('CopyAll', ':%y+', {})
vim.api.nvim_create_user_command('Highlights', ':Telescope highlights', {})
vim.api.nvim_create_user_command('LazyProfile', ':Lazy profile', {})
vim.api.nvim_create_user_command('LazyUpdate', ':Lazy update', {})
vim.api.nvim_create_user_command('SelectAll', 'normal! ggVG', {})
vim.api.nvim_create_user_command('Messages', ':Messages', {})

vim.api.nvim_create_autocmd('TextYankPost', {
  group = vim.api.nvim_create_augroup('YankHighlight', { clear = true, }),
  pattern = '*',
  callback = function() vim.highlight.on_yank() end,
})

vim.api.nvim_create_autocmd({ 'BufLeave', 'FocusLost', }, {
  group = vim.api.nvim_create_augroup('AutosaveBuffers', { clear = true, }),
  command = ':silent! wa!',
})

vim.api.nvim_create_autocmd({ 'FileType', }, {
  group = vim.api.nvim_create_augroup('QuickfixConfig', { clear = true, }),
  pattern = 'qf',
  callback = function() require('keymaps').quickfix() end,
})
