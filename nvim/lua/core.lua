local vg = vim.g
local vo = vim.o
local vopt = vim.opt
local vwo = vim.wo

vg.mapleader = ' '
vg.maplocalleader = ' '

for _, provider in ipairs { "node", "perl", "python3", "ruby" } do
  vg["loaded_" .. provider .. "_provider"] = 0
end

vo.autoindent = true
vo.backspace = 'indent,eol,start'
vo.breakindent = true
vo.colorcolumn = '120'
vo.completeopt = 'menuone,noselect'
vo.cursorline = true
vo.expandtab = true
vo.hlsearch = true
vo.ignorecase = true
vo.list = true
vo.mouse = 'a'
vo.number = true
vo.shiftwidth = 2
vo.sidescroll = 1
vo.signcolumn = 'yes'
vo.smartcase = true
vo.splitbelow = true
vo.splitright = true
vo.tabstop = 2
vo.termguicolors = true
vo.undofile = true
vo.updatetime = 250
vo.wrap = false
vopt.clipboard:append('unnamedplus')
vopt.iskeyword:append('-')
vopt.shortmess:append('sI')
vwo.number = true
vwo.signcolumn = 'yes'
