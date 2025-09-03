vim.loader.enable()

local function rua_lib()
  return os.getenv('HOME') ..
      '/data/dev/dotfiles/dotfiles/yog/target/' ..
      (vim.env.DEBUG_RUA and 'debug' or 'release') ..
      '/?.so'
end

package.cpath = package.cpath .. ';' .. rua_lib()

local rua = require('rua')

vim.g.mapleader = ' '
vim.g.maplocalleader = ' '
require('keymaps').setup()
rua.set_vim_opts()
rua.set_colorscheme()
rua.create_cmds()

require('diagnostics').setup(rua)

for _, provider in ipairs { 'node', 'perl', 'python3', 'ruby', } do
  vim.g['loaded_' .. provider .. '_provider'] = 0
end
