return {
  'saghen/blink.pairs',
  build = 'cargo build --release',
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
