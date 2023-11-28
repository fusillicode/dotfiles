return {
  'lewis6991/gitsigns.nvim',
  event = 'BufReadPost',
  opts = {
    on_attach = function(_)
      local gs = package.loaded.gitsigns
      local vkeyset = vim.keymap.set
      local vwod = vim.wo.diff
      local vsched = vim.schedule

      vkeyset('n', 'cn', function()
        if vwod then return 'cn' end
        vsched(function() gs.next_hunk({ wrap = true, }) end)
        return '<Ignore>'
      end, { expr = true, })

      vkeyset('n', 'cp', function()
        if vwod then return 'cp' end
        vsched(function() gs.prev_hunk({ wrap = true, }) end)
        return '<Ignore>'
      end, { expr = true, })

      vkeyset('n', '<leader>hs', gs.stage_hunk)
      vkeyset('n', '<leader>hr', gs.reset_hunk)
      vkeyset('v', '<leader>hs', function() gs.stage_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
      vkeyset('v', '<leader>hr', function() gs.reset_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
      vkeyset('n', '<leader>hu', gs.undo_stage_hunk)
      vkeyset('n', '<leader>tb', function() gs.blame_line({ full = true, }) end)
    end,
  },
}
