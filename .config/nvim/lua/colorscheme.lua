local M = {}

local bg = '#001900'
local set_hl = vim.api.nvim_set_hl
local get_hl = vim.api.nvim_get_hl

function M.setup(colorscheme)
  if colorscheme then vim.cmd.colorscheme(colorscheme) end
  vim.o.background = 'dark'
  vim.o.termguicolors = true
  require("rua").set_colorscheme()
end

M.window = { border = 'rounded', }

return M
