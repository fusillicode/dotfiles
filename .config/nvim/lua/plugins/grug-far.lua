return {
  'MagicDuck/grug-far.nvim',
  keys = { { '<leader>l', mode = { 'n', 'v', }, }, '<leader>/', },
  config = function()
    local grug_far = require('grug-far')

    local opts = {
      engines = {
        ripgrep = {
          extraArgs =
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
          placeholders = {
            enabled = false,
          },
        },
      },
      disableBufferLineNumbers = true,
      icons = {
        enabled = false,
      },
      folding = {
        enabled = false,
      },
      resultLocation = {
        showNumberLabel = false,
      },
      keymaps = {
        swapReplacementInterpreter = { n = '<localleader>v', },
      },
    }

    grug_far.setup(opts)

    require('keymaps').grug_far(grug_far.grug_far, opts)
  end,
}
