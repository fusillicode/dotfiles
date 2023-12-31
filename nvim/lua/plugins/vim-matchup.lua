return {
  'andymass/vim-matchup',
  event = 'BufEnter',
  config = function()
    vim.g.matchup_matchparen_offscreen = {}
  end,
}
