vim.loader.enable()

local nvrim = require('nvrim')
nvrim.vim_opts.set_all()
nvrim.cmds.create()
nvrim.keymaps.set_all()
require('keymaps').set_lua_defined()
require('diagnostics').setup(nvrim)

for _, provider in ipairs { 'node', 'perl', 'python3', 'ruby', } do
  vim.g['loaded_' .. provider .. '_provider'] = 0
end

local lazypath = vim.fn.stdpath('data') .. '/lazy/lazy.nvim'
if not (vim.uv or vim.loop).fs_stat(lazypath) then
  vim.fn.system({
    'git',
    'clone',
    '--filter=blob:none',
    '--branch=stable',
    'https://github.com/folke/lazy.nvim',
    lazypath,
  })
end
vim.opt.rtp:prepend(lazypath)

require('lazy').setup('plugins', {
  change_detection = { notify = false, },
  performance = {
    rtp = {
      disabled_plugins = {
        '2html_plugin',
        'bugreport',
        'compiler',
        'getscript',
        'getscriptPlugin',
        'gzip',
        'logipat',
        'matchit',
        'netrw',
        'netrwFileHandlers',
        'netrwPlugin',
        'netrwSettings',
        'optwin',
        'rplugin',
        'rrhelper',
        'spellfile',
        'synmenu',
        'syntax',
        'tar',
        'tarPlugin',
        'tohtml',
        'tutor',
        'vimball',
        'vimballPlugin',
        'zip',
        'zipPlugin',
      },
    },
  },
})

-- Colorscheme are installed via lazy so this must be called after they are actually installed
nvrim.colorscheme.set()
