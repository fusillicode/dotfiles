vim.loader.enable()

local function nvrim_lib()
  return os.getenv('HOME') ..
      '/data/dev/dotfiles/dotfiles/yog/target/' ..
      (vim.env.NVRIM_DEBUG and 'debug' or 'release') ..
      '/?.so'
end

package.cpath = package.cpath .. ';' .. nvrim_lib()

local nvrim = require('nvrim')

nvrim.vim_opts.set_all()
nvrim.colorscheme.set()
nvrim.cmds.create()
nvrim.keymaps.set_all()
require('keymaps').set_lua_implemented()
require('diagnostics').setup(nvrim)

for _, provider in ipairs { 'node', 'perl', 'python3', 'ruby', } do
  vim.g['loaded_' .. provider .. '_provider'] = 0
end
