return {
  'saghen/blink.cmp',
  event = 'InsertEnter',
  build = 'cargo build --release',
  dependencies = {
    'L3MON4D3/LuaSnip',
  },
  opts = {
    appearance = {
      use_nvim_cmp_as_default = true,
    },
    completion = {
      documentation = {
        auto_show = true,
        auto_show_delay_ms = 0,
        window = { border = 'single', },
      },
      menu = {
        border = 'single',
        draw = {
          columns = {
            { 'source_name', gap = 1, 'label', },
          },
          components = {
            source_name = {
              text = function(ctx) return '[' .. ctx.source_name .. ']' end,
            },
          },
        },
      },
    },
    keymap = {
      preset = 'enter',
      -- For some reason <c-space> does't work...
      ['<c-x>'] = { 'show', 'show_documentation', 'hide_documentation', },
      ['<c-u>'] = { 'scroll_documentation_up', 'fallback', },
      ['<c-d>'] = { 'scroll_documentation_down', 'fallback', },
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
