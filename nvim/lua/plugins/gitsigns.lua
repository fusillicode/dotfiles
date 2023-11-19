return {
  'lewis6991/gitsigns.nvim',
  event = 'BufReadPost',
  opts = {
    on_attach = function(_)
      local gs = package.loaded.gitsigns
      local vkeyset = vim.keymap.set

      vkeyset('n', ']c', function()
        if vim.wo.diff then return ']c' end
        vim.schedule(function() gs.next_hunk() end)
        return '<Ignore>'
      end, { expr = true })

      vkeyset('n', '[c', function()
        if vim.wo.diff then return '[c' end
        vim.schedule(function() gs.prev_hunk() end)
        return '<Ignore>'
      end, { expr = true })

      vkeyset('n', '<leader>hs', gs.stage_hunk)
      vkeyset('n', '<leader>hr', gs.reset_hunk)
      vkeyset('v', '<leader>hs', function() gs.stage_hunk({ vim.fn.line('.'), vim.fn.line('v') }) end)
      vkeyset('v', '<leader>hr', function() gs.reset_hunk({ vim.fn.line('.'), vim.fn.line('v') }) end)
      vkeyset('n', '<leader>hu', gs.undo_stage_hunk)
      vkeyset('n', '<leader>tb', gs.toggle_current_line_blame)
      vkeyset('n', '<leader>td', gs.toggle_deleted)
    end
  }
}
