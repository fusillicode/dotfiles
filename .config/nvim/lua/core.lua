vim.loader.enable()

local function rua_lib()
  return os.getenv('HOME') ..
      '/data/dev/dotfiles/dotfiles/yog/target/' ..
      (vim.env.DEBUG_RUA and 'debug' or 'release') ..
      '/?.so'
end

package.cpath = package.cpath .. ';' .. rua_lib()

local rua = require('rua');

require('commands').setup(rua)
require('diagnostics').setup(rua)
require('keymaps').setup()
rua.set_highlights()

for _, provider in ipairs { 'node', 'perl', 'python3', 'ruby', } do
  vim.g['loaded_' .. provider .. '_provider'] = 0
end

rua.set_vim_opts();
