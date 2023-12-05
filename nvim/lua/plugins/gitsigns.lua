return {
  'lewis6991/gitsigns.nvim',
  event = 'BufReadPost',
  opts = {
    on_attach = function(_)
      local gs = package.loaded.gitsigns

      vim.keymap.set('n', 'cn', function()
        if vim.wo.diff then return 'cn' end
        vim.schedule(function() gs.next_hunk({ wrap = true, }) end)
        return '<Ignore>'
      end, { expr = true, })

      vim.keymap.set('n', 'cp', function()
        if vim.wo.diff then return 'cp' end
        vim.schedule(function() gs.prev_hunk({ wrap = true, }) end)
        return '<Ignore>'
      end, { expr = true, })

      vim.keymap.set('n', '<leader>hs', gs.stage_hunk)
      vim.keymap.set('n', '<leader>hr', gs.reset_hunk)
      vim.keymap.set('v', '<leader>hs', function() gs.stage_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
      vim.keymap.set('v', '<leader>hr', function() gs.reset_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
      vim.keymap.set('n', '<leader>hu', gs.undo_stage_hunk)
      vim.keymap.set('n', '<leader>tb', function() gs.blame_line({ full = true, }) end)
    end,
  },
}
