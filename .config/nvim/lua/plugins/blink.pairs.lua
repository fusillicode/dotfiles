return {
  'saghen/blink.pairs',
  build = function() require('blink.pairs').build():pwait(60000) end,
  event = 'InsertEnter',
  opts = {
    mappings = {
      enabled = true,
      cmdline = true,
      disabled_filetypes = {},
      pairs = {},
    },
    highlights = {
      enabled = false,
      cmdline = false,
      matchparen = {
        enabled = true,
        cmdline = false,
        include_surrounding = false,
        group = 'BlinkPairsMatchParen',
        priority = 250,
      },
    },
  },
}
