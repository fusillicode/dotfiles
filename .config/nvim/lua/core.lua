vim.loader.enable()

local function nvrim_lib()
  return os.getenv('HOME') ..
      '/data/dev/dotfiles/dotfiles/yog/target/' ..
      (vim.env.NVRIM_DEBUG and 'debug' or 'release') ..
      '/?.so'
end

package.cpath = package.cpath .. ';' .. nvrim_lib()

local nvrim = require('nvrim')

require('keymaps').set_all()
nvrim.keymaps.set_all()
nvrim.vim_opts.set_all()
nvrim.colorscheme.set()
nvrim.cmds.create()

require('diagnostics').setup(nvrim)

for _, provider in ipairs { 'node', 'perl', 'python3', 'ruby', } do
  vim.g['loaded_' .. provider .. '_provider'] = 0
end
