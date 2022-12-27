local ensure_packer = function()
  local fn = vim.fn
  local install_path = fn.stdpath('data') .. '/site/pack/packer/start/packer.nvim'
  if fn.empty(fn.glob(install_path)) > 0 then
    fn.system({ 'git', 'clone', '--depth', '1', 'https://github.com/wbthomason/packer.nvim', install_path })
    vim.cmd([[packadd packer.nvim]])
    return true
  end
  return false
end

local packer_bootstrap = ensure_packer()

require('packer').startup(function(use)
  use 'wbthomason/packer.nvim'
  use 'jim-at-jibba/ariake-vim-colors'
  use {'nvim-telescope/telescope.nvim', requires = {'nvim-lua/plenary.nvim'}}
  use 'lewis6991/gitsigns.nvim'
  use 'Pocco81/auto-save.nvim'
  use 'jghauser/mkdir.nvim'
  use 'cappyzawa/trim.nvim'

  if packer_bootstrap then
    require('packer').sync()
    return
  end
end)
