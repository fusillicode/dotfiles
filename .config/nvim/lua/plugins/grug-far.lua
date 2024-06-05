return {
  'MagicDuck/grug-far.nvim',
  keys = { { '<leader>w', mode = { 'n', 'v', }, }, '<leader>/', },
  config = function()
    local grug_far = require('grug-far')

    local opts = {
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
    }

    grug_far.setup(opts)

    require('keymaps').grug_far(grug_far.grug_far, opts)
  end,
}
