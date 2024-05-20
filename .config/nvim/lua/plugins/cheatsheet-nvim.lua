return {
  'doctorfree/cheatsheet.nvim',
  keys = { '<leader>', 'c', },
  dependencies = {
    { 'nvim-telescope/telescope.nvim', },
    { 'nvim-lua/plenary.nvim', },
  },
  config = function()
    local cheatsheet_telescope_actions = require('cheatsheet.telescope.actions')

    require('cheatsheet').setup({
      bundled_cheetsheets = {
        disabled = { 'nerd-fonts', 'unicode', 'netrw', },
      },
      include_only_installed_plugins = true,
      telescope_mappings = {
        ['<cr>'] = cheatsheet_telescope_actions.select_or_execute,
      },
    })
  end,
}
