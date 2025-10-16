local M = {}

local statusline = require('nvrim').statusline

-- Called from vim.opt.statusline (Rust side). We'll see if I can get rid of this at some point...
function M.draw() return statusline.draw(vim.diagnostic.get()) end

vim.api.nvim_create_autocmd(statusline.draw_triggers, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  callback = function() vim.o.statusline = statusline.draw(vim.diagnostic.get()) end,
})

return M
