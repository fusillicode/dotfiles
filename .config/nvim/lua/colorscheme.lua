local M = {}

function M.setup(colorscheme)
  if colorscheme then vim.cmd.colorscheme(colorscheme) end
  vim.o.background = 'dark'
  vim.o.termguicolors = true
  require('rua').set_highlights()
end

M.window = { border = 'rounded', }

return M
