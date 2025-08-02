vim.api.nvim_create_user_command('CopyAll', ':%y+', {})
vim.api.nvim_create_user_command('Highlights', ':FzfLua highlights', {})
vim.api.nvim_create_user_command('LazyProfile', ':Lazy profile', {})
vim.api.nvim_create_user_command('LazyUpdate', ':Lazy update', {})
vim.api.nvim_create_user_command('SelectAll', 'normal! ggVG', {})
vim.api.nvim_create_user_command('Messages', ':Messages', {})

local rua = require('rua')
local utils = require('utils')
for _, cmd in ipairs(rua.get_fkr_cmds()) do
  vim.api.nvim_create_user_command(cmd.name, function()
    local row, col = utils.unpack(vim.api.nvim_win_get_cursor(0))
    vim.api.nvim_buf_set_text(0, row - 1, col, row - 1, col, { rua.gen_fkr_value(cmd.fkr_arg), })
  end, {})
end

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
