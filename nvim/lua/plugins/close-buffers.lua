return {
  'kazhala/close-buffers.nvim',
  event = 'BufReadPost',
  config = function()
    local close_buffers = require('close_buffers')

    vim.keymap.set('n', '<leader>o', function() close_buffers.wipe({ type = 'other', }) end)
    vim.keymap.set('n', '<leader>O', function() close_buffers.wipe({ type = 'other', force = true, }) end)
  end,
}
