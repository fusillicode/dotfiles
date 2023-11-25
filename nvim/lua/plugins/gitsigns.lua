return {
  'lewis6991/gitsigns.nvim',
  event = 'BufReadPost',
  opts = {
    on_attach = function(_)
      local gs = package.loaded.gitsigns
      local vkeyset = vim.keymap.set
      local vwod = vim.wo.diff
      local vsched = vim.schedule

      vkeyset('n', ']c', function()
        if vwod then return ']c' end
        vsched(function() gs.next_hunk() end)
        return '<Ignore>'
      end, { expr = true, })

      vkeyset('n', '[c', function()
        if vwod then return '[c' end
        vsched(function() gs.prev_hunk() end)
        return '<Ignore>'
      end, { expr = true, })

      vkeyset('v', '<leader>hs', function() gs.stage_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
      vkeyset('v', '<leader>hr', function() gs.reset_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
      vkeyset('n', '<leader>hu', gs.undo_stage_hunk)
      vkeyset('n', '<leader>tb', function() gs.blame_line({ full = true, }) end)
    end,
  },
}
