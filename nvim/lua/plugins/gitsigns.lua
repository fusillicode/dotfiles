return {
  'lewis6991/gitsigns.nvim',
  event = 'BufReadPost',
  opts = {
    on_attach = function(_)
      local gs = package.loaded.gitsigns
      local keymap_set = require('utils').keymap_set

      keymap_set('n', 'cn', function()
        if vim.wo.diff then return 'cn' end
        vim.schedule(function() gs.next_hunk({ wrap = true, }) end)
        return '<Ignore>'
      end, { expr = true, })

      keymap_set('n', 'cp', function()
        if vim.wo.diff then return 'cp' end
        vim.schedule(function() gs.prev_hunk({ wrap = true, }) end)
        return '<Ignore>'
      end, { expr = true, })

      keymap_set('n', '<leader>hs', gs.stage_hunk)
      keymap_set('n', '<leader>hr', gs.reset_hunk)
      keymap_set('v', '<leader>hs', function() gs.stage_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
      keymap_set('v', '<leader>hr', function() gs.reset_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
      keymap_set('n', '<leader>hu', gs.undo_stage_hunk)
      keymap_set('n', '<leader>tb', function() gs.blame_line({ full = true, }) end)
    end,
  },
}
