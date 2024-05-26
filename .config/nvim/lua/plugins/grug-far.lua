return {
  'MagicDuck/grug-far.nvim',
  keys = { '<leader>w', '<leader>/', },
  config = function()
    local grug_far = require('grug-far')

    grug_far.setup({
      extraRgArgs =
          '--color=never' ..
          ' --column' ..
          ' --line-number' ..
          ' --no-heading' ..
          ' --smart-case' ..
          ' --with-filename' ..
          ' --hidden' ..
          ' --glob=!**/.git/*' ..
          ' --glob=!**/target/*' ..
          ' --glob=!**/_build/*' ..
          ' --glob=!**/deps/*' ..
          ' --glob=!**/.elixir_ls/*' ..
          ' --glob=!**/.node_modules/*',
      icons = {
        enabled = false,
      },
      headerMaxWidth = 0,
      placeholders = {
        enabled = false,
      },
      prefills = {
        filesFilter = '**/*',
      },
    })

    require('keymaps').grug_far(grug_far)
  end,
}
