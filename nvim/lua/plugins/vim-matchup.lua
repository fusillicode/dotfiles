return {
  'andymass/vim-matchup',
  event = 'InsertEnter',
  config = function()
    vim.g.matchup_matchparen_offscreen = {}
  end,
}
