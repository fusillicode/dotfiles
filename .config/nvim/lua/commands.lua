vim.api.nvim_create_user_command('CopyAll', ':%y+', {})
vim.api.nvim_create_user_command('LazyProfile', ':Lazy profile', {})
vim.api.nvim_create_user_command('LazyUpdate', ':Lazy update', {})
vim.api.nvim_create_user_command('SelectAll', 'normal! ggVG', {})
