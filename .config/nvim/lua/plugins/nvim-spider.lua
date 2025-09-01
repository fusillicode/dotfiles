local keymaps = require('keymaps')
local nvim_spider_keymaps = keymaps.nvim_spider

return {
  'chrisgrieser/nvim-spider',
  keys = keymaps.build(nvim_spider_keymaps),
  config = function()
    local spider = require('spider')
    keymaps.build(nvim_spider_keymaps, spider)
    spider.setup({})
  end,
}
