return {
  'saghen/blink.cmp',
  event = 'InsertEnter',
  build = 'cargo build --release',
  dependencies = {
    'L3MON4D3/LuaSnip',
  },
  opts = {
    keymap = { preset = 'enter', },
    completion = {
      documentation = {
        auto_show = true,
        auto_show_delay_ms = 0,
        window = { border = 'single', },
      },
      menu = { border = 'single', },
    },
    signature = { enabled = true, },
    snippets = {
      expand = function(snippet) require('luasnip').lsp_expand(snippet) end,
      active = function(filter)
        if filter and filter.direction then
          return require('luasnip').jumpable(filter.direction)
        end
        return require('luasnip').in_snippet()
      end,
      jump = function(direction) require('luasnip').jump(direction) end,
    },
  },
}
