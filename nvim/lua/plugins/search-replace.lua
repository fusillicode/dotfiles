return {
  'roobert/search-replace.nvim',
  keys = { '<leader>', mode = { 'n', 'v', }, },
  config = function()
    local keymap_set = require('utils').keymap_set

    keymap_set('v', '<C-r>', '<cmd>SearchReplaceSingleBufferVisualSelection<cr>')
    keymap_set('v', '<C-s>', '<cmd>SearchReplaceWithinVisualSelection<cr>')
    keymap_set('v', '<C-b>', '<cmd>SearchReplaceWithinVisualSelectionCWord<cr>')

    keymap_set('n', '<leader>rs', '<cmd>SearchReplaceSingleBufferSelections<cr>')
    keymap_set('n', '<leader>ro', '<cmd>SearchReplaceSingleBufferOpen<cr>')
    keymap_set('n', '<leader>rw', '<cmd>SearchReplaceSingleBufferCWord<cr>')
    keymap_set('n', '<leader>rW', '<cmd>SearchReplaceSingleBufferCWORD<cr>')
    keymap_set('n', '<leader>re', '<cmd>SearchReplaceSingleBufferCExpr<cr>')
    keymap_set('n', '<leader>rf', '<cmd>SearchReplaceSingleBufferCFile<cr>')

    keymap_set('n', '<leader>rbs', '<cmd>SearchReplaceMultiBufferSelections<cr>')
    keymap_set('n', '<leader>rbo', '<cmd>SearchReplaceMultiBufferOpen<cr>')
    keymap_set('n', '<leader>rbw', '<cmd>SearchReplaceMultiBufferCWord<cr>')
    keymap_set('n', '<leader>rbW', '<cmd>SearchReplaceMultiBufferCWORD<cr>')
    keymap_set('n', '<leader>rbe', '<cmd>SearchReplaceMultiBufferCExpr<cr>')
    keymap_set('n', '<leader>rbf', '<cmd>SearchReplaceMultiBufferCFile<cr>')

    require('search-replace').setup({})
  end,
}
