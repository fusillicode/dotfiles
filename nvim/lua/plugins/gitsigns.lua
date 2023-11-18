return {
  'lewis6991/gitsigns.nvim',
  event = 'BufReadPost',
  opts = {
    on_attach = function(_)
      local gs = package.loaded.gitsigns

      vim.keymap.set('n', ']c', function()
        if vim.wo.diff then return ']c' end
        vim.schedule(function() gs.next_hunk() end)
        return '<Ignore>'
      end, { expr = true })

      vim.keymap.set('n', '[c', function()
        if vim.wo.diff then return '[c' end
        vim.schedule(function() gs.prev_hunk() end)
        return '<Ignore>'
      end, { expr = true })

      vim.keymap.set('n', '<leader>hs', gs.stage_hunk)
      vim.keymap.set('n', '<leader>hr', gs.reset_hunk)
      vim.keymap.set('v', '<leader>hs', function() gs.stage_hunk({ vim.fn.line('.'), vim.fn.line('v') }) end)
      vim.keymap.set('v', '<leader>hr', function() gs.reset_hunk({ vim.fn.line('.'), vim.fn.line('v') }) end)
      vim.keymap.set('n', '<leader>hu', gs.undo_stage_hunk)
      vim.keymap.set('n', '<leader>tb', gs.toggle_current_line_blame)
      vim.keymap.set('n', '<leader>td', gs.toggle_deleted)
    end
  }
}
