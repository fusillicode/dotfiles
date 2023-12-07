return {
  'kazhala/close-buffers.nvim',
  event = 'BufReadPost',
  config = function()
    local close_buffers = require('close_buffers')
    local keymap_set = require('utils').keymap_set

    keymap_set('n', '<leader>o', function() close_buffers.wipe({ type = 'other', }) end)
    keymap_set('n', '<leader>O', function() close_buffers.wipe({ type = 'other', force = true, }) end)
  end,
}
