vim.api.nvim_create_user_command('CopyAll', ':%y+', {})
vim.api.nvim_create_user_command('LazyProfile', ':Lazy profile', {})
vim.api.nvim_create_user_command('LazyUpdate', ':Lazy update', {})
vim.api.nvim_create_user_command('SelectAll', 'normal! ggVG', {})

vim.api.nvim_create_user_command('SRSearch', function()
  local search = vim.fn.getreg('/')
  if not search or search == '' then
    print('No search term in register /')
    return
  end

  local replace = vim.fn.input('Replace: ')
  if not replace or replace == '' then
    print('No replace provided')
    return
  end

  vim.api.nvim_command(':%s/' .. search .. '/' .. replace .. '/g')
end, {})

vim.api.nvim_create_user_command('SRBuf', function()
  local search = vim.fn.input('Search: ')
  if not search or search == '' then
    print('No search provided')
    return
  end

  local replace = vim.fn.input('Replace: ')
  if not replace or replace == '' then
    print('No replace provided')
    return
  end

  vim.api.nvim_command(':%s/' .. search .. '/' .. replace .. '/g')
end, {})
