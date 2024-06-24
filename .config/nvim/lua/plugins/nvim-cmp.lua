return {
  'hrsh7th/nvim-cmp',
  event = 'InsertEnter',
  dependencies = {
    'L3MON4D3/LuaSnip',
    'hrsh7th/cmp-buffer',
    'hrsh7th/cmp-nvim-lsp',
    'hrsh7th/cmp-nvim-lsp-signature-help',
    'hrsh7th/cmp-path',
    'lukas-reineke/cmp-rg',
    'rafamadriz/friendly-snippets',
    'saadparwaiz1/cmp_luasnip',
    'davidsierradz/cmp-conventionalcommits',
    'Exafunction/codeium.nvim',
  },
  config = function()
    local cmp = require('cmp')
    local luasnip = require('luasnip')

    require('luasnip.loaders.from_vscode').load({ paths = { './snippets', }, })
    require('codeium').setup({})

    cmp.event:on('confirm_done', require('nvim-autopairs.completion.cmp').on_confirm_done())

    local ok, ika = pcall(require, 'ika')
    if ok then
      local ika_source = {}
      function ika_source:complete(params, callback)
        ika.complete({ params = params, callback = callback, })
      end

      cmp.register_source('ika', ika_source)
    end

    cmp.setup({
      experimental = { ghost_text = true, },
      formatting = {
        format = function(entry, vim_item)
          vim_item.kind = ' '
          vim_item.menu = entry.source.name
          vim_item.abbr = vim_item.abbr:match('[^(]+')
          return vim_item
        end,
      },
      performance = { max_view_entries = 12, },
      snippet = { expand = function(args) luasnip.lsp_expand(args.body) end, },
      mapping = cmp.mapping.preset.insert({
        ['<c-d>'] = cmp.mapping.scroll_docs(-4),
        ['<c-u>'] = cmp.mapping.scroll_docs(4),
        ['<c-space>'] = cmp.mapping.complete(),
        ['<cr>'] = cmp.mapping.confirm({ select = true, }),
        ['<tab>'] = cmp.mapping(function(fallback)
          if cmp.visible() then
            cmp.select_next_item()
          elseif luasnip.expand_or_jumpable() then
            luasnip.expand_or_jump()
          else
            fallback()
          end
        end, { 'i', 's', }),
        ['<s-tab>'] = cmp.mapping(function(fallback)
          if cmp.visible() then
            cmp.select_prev_item()
          elseif luasnip.jumpable(-1) then
            luasnip.jump(-1)
          else
            fallback()
          end
        end, { 'i', 's', }),
      }),
      sources = {
        { name = 'ika', },
        { name = 'nvim_lsp', },
        { name = 'nvim_lsp_signature_help', },
        { name = 'codeium', },
        { name = 'path', },
        { name = 'buffer', },
        { name = 'luasnip', },
        { name = 'crates', },
        { name = 'rg',                      keyword_length = 3, },
      },
      entries = { follow_cursor = true, },
      window = {
        completion = cmp.config.window.bordered(),
        documentation = cmp.config.window.bordered(),
      },
    })
  end,
}
