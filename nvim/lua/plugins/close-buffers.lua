return {
  'kazhala/close-buffers.nvim',
  event = 'BufReadPost',
  config = function()
    require('keymaps').close_buffers(require('close_buffers'))
  end,
}
