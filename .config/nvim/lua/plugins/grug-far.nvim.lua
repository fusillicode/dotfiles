local keymaps = require('keymaps')
local plugin_keymaps = keymaps.grug_far

return {
  'MagicDuck/grug-far.nvim',
  keys = plugin_keymaps(),
  config = function()
    local plugin = require('grug-far')

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

    plugin.setup(opts)
    keymaps.set(plugin_keymaps(plugin, opts))
  end,
}
