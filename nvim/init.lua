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

local is_packer_boostrapped = ensure_packer()

require('packer').startup(function(use)
  use 'wbthomason/packer.nvim'
  use {
    'neovim/nvim-lspconfig',
    requires = {
      'williamboman/mason.nvim',
      'williamboman/mason-lspconfig.nvim',
      'j-hui/fidget.nvim',
      'folke/neodev.nvim',
    },
  }
  use {
    'hrsh7th/nvim-cmp',
    requires = {
      'hrsh7th/cmp-nvim-lsp',
      'hrsh7th/cmp-buffer',
      'hrsh7th/cmp-path',
      'saadparwaiz1/cmp_luasnip',
      'L3MON4D3/LuaSnip',
    },
  }
  use {
    'nvim-treesitter/nvim-treesitter',
    run = function() pcall(require('nvim-treesitter.install').update { with_sync = true }) end,
  }
  use { 'nvim-treesitter/nvim-treesitter-textobjects', after = 'nvim-treesitter' }
  use 'lewis6991/gitsigns.nvim'
  use 'nvim-lualine/lualine.nvim'
  use { 'folke/tokyonight.nvim', branch = 'main' }
  use 'numToStr/Comment.nvim'
  use { 'nvim-telescope/telescope.nvim', branch = '0.1.x', requires = { 'nvim-lua/plenary.nvim' } }
  use 'ahmedkhalf/project.nvim'
  use { 'saecki/crates.nvim', requires = { 'nvim-lua/plenary.nvim' } }
  use 'bogado/file-line'
  use 'mg979/vim-visual-multi'
  use 'kdarkhan/rust-tools.nvim'
  use 'Pocco81/auto-save.nvim'
  use 'nvim-tree/nvim-tree.lua'
  use 'windwp/nvim-autopairs'
  use 'windwp/nvim-spectre'

  if is_packer_boostrapped then
    require('packer').sync()
  end
end)

if is_packer_boostrapped then
  print 'Packer not installed :('
  return
end

require('tokyonight').setup({ style = 'night' })

vim.cmd('autocmd BufWritePre * lua vim.lsp.buf.formatting_sync()')
vim.cmd('colorscheme tokyonight')
vim.g.loaded_netrw = 1
vim.g.loaded_netrwPlugin = 1
vim.g.mapleader = ' '
vim.g.maplocalleader = ' '
vim.o.autoindent = true
vim.o.background = 'dark'
vim.o.backspace = 'indent,eol,start'
vim.o.breakindent = true
vim.o.colorcolumn = '120'
vim.o.completeopt = 'menuone,noselect'
vim.o.cursorline = true
vim.o.expandtab = true
vim.o.guicursor = ''
vim.o.hlsearch = true
vim.o.ignorecase = true
vim.o.list = true
vim.o.mouse = 'a'
vim.o.number = true
vim.o.shiftwidth = 2
vim.o.signcolumn = 'yes'
vim.o.smartcase = true
vim.o.splitbelow = true
vim.o.splitright = true
vim.o.tabstop = 2
vim.o.termguicolors = true
vim.o.undofile = true
vim.o.updatetime = 250
vim.o.wrap = true
vim.opt.clipboard:append('unnamedplus')
vim.opt.iskeyword:append('-')
vim.wo.number = true
vim.wo.signcolumn = 'yes'

vim.keymap.set({ 'n', 'v' }, '<leader>', '<Nop>')
vim.keymap.set('v', '>', '>gv')
vim.keymap.set('v', '<', '<gv')
vim.keymap.set('n', '<esc>', ':noh <CR>', {})
require('telescope').load_extension('projects')
vim.keymap.set('n', '<leader>fp', ':Telescope projects <CR>', {})
vim.keymap.set('n', '<leader>fo', require('telescope.builtin').oldfiles)
vim.keymap.set('n', '<leader>fb', require('telescope.builtin').buffers)
vim.keymap.set('n', '<leader>ff', require('telescope.builtin').find_files)
vim.keymap.set('n', '<leader>gc', require('telescope.builtin').git_commits)
vim.keymap.set('n', '<leader>gbc', require('telescope.builtin').git_bcommits)
vim.keymap.set('n', '<leader>gb', require('telescope.builtin').git_branches)
vim.keymap.set('n', '<leader>gs', require('telescope.builtin').git_status)
vim.keymap.set('n', '<leader>rf', ':NvimTreeFindFileToggle <CR>', {})
vim.keymap.set('n', '<leader>e', vim.diagnostic.open_float)
vim.keymap.set('n', '<leader>s', require('spectre').open_visual)
vim.keymap.set('n', '[d', vim.diagnostic.goto_prev)
vim.keymap.set('n', ']d', vim.diagnostic.goto_next)
vim.cmd([[
  let g:VM_maps = {}
  let g:VM_maps['Add Cursor Down'] = '<C-j>'
  let g:VM_maps['Add Cursor Up'] = '<C-k>'
]])

local lsp_on_attach = function(_, bufnr)
  vim.keymap.set('n', 'gd', vim.lsp.buf.definition, { buffer = bufnr })
  vim.keymap.set('n', 'gD', vim.lsp.buf.declaration, { buffer = bufnr })
  vim.keymap.set('n', 'gr', require('telescope.builtin').lsp_references, { buffer = bufnr })
  vim.keymap.set('n', 'gi', vim.lsp.buf.implementation, { buffer = bufnr })
  vim.keymap.set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr })
  vim.keymap.set('n', '<C-h>', vim.lsp.buf.signature_help, { buffer = bufnr })
  -- vim.keymap.set('n', '<leader>f', function() vim.lsp.buf.format { async = true } end)
  vim.keymap.set('n', '<leader>rn', vim.lsp.buf.rename, { buffer = bufnr })
  vim.keymap.set('n', '<leader>ca', vim.lsp.buf.code_action, { buffer = bufnr })
  vim.api.nvim_buf_create_user_command(bufnr, 'Format', function(_) vim.lsp.buf.format() end, {})
end

local highlight_group = vim.api.nvim_create_augroup('YankHighlight', { clear = true })
vim.api.nvim_create_autocmd('TextYankPost', {
  callback = function() vim.highlight.on_yank() end,
  group = highlight_group,
  pattern = '*',
})

require('lualine').setup {
  options = {
    icons_enabled = false,
    theme = 'auto',
    component_separators = '',
    section_separators = '',
  },
  sections = {
    lualine_a = {},
    lualine_b = {},
    lualine_c = { 'searchcount', 'diagnostics', { 'filename', file_status = true, path = 1 }, 'encoding' },
    lualine_x = { { 'branch', fmt = function(str) return str:sub(1, 33) end } },
    lualine_y = {},
    lualine_z = {}
  },
}

require('Comment').setup({})
require('gitsigns').setup({})
require('project_nvim').setup({
  detection_methods = { 'pattern' },
  show_hidden = false,
})

require('nvim-treesitter.configs').setup {
  ensure_installed = { 'rust', 'lua', 'python', 'help', 'vim' },
  sync_install = true,
  auto_install = false,

  highlight = { enable = true },
  incremental_selection = {
    enable = true,
    keymaps = {
      init_selection = '<C-Space>',
      node_incremental = '<C-Space>',
      scope_incremental = '<C-s>',
      node_decremental = '<C-Backspace>',
    },
  },
  textobjects = {
    select = {
      enable = true,
      lookahead = true,
      keymaps = {
        ['aa'] = '@parameter.outer',
        ['ia'] = '@parameter.inner',
        ['af'] = '@function.outer',
        ['if'] = '@function.inner',
        ['ac'] = '@class.outer',
        ['ic'] = '@class.inner',
      },
    },
    move = {
      enable = true,
      set_jumps = true,
      goto_next_start = {
        [']m'] = '@function.outer',
        [']]'] = '@class.outer',
      },
      goto_next_end = {
        [']M'] = '@function.outer',
        [']['] = '@class.outer',
      },
      goto_previous_start = {
        ['[m'] = '@function.outer',
        ['[['] = '@class.outer',
      },
      goto_previous_end = {
        ['[M'] = '@function.outer',
        ['[]'] = '@class.outer',
      },
    },
    swap = {
      enable = true,
      swap_next = {
        ['<leader>a'] = '@parameter.inner',
      },
      swap_previous = {
        ['<leader>A'] = '@parameter.inner',
      },
    },
  },
}

local lsp_servers = {
  pyright = {},
  rust_analyzer = {
    ['rust-analyzer'] = {
      imports = {
        granularity = {
          group = 'item',
        },
        prefix = 'crate',
      },
      checkOnSave = 'clippy'
    }
  },
  tsserver = {},
  sumneko_lua = {
    Lua = {
      workspace = { checkThirdParty = false },
      telemetry = { enable = false },
      diagnostics = {
        globals = { 'vim' }
      }
    },
  },
}

require('neodev').setup({})

require('mason').setup({})

local mason_lspconfig = require 'mason-lspconfig'
mason_lspconfig.setup {
  ensure_installed = vim.tbl_keys(lsp_servers),
}

local capabilities = vim.lsp.protocol.make_client_capabilities()
capabilities = require('cmp_nvim_lsp').default_capabilities(capabilities)

mason_lspconfig.setup_handlers {
  function(server_name)
    require('lspconfig')[server_name].setup {
      capabilities = capabilities,
      on_attach = lsp_on_attach,
      settings = lsp_servers[server_name],
    }
  end,
}

require('fidget').setup({
  align = {
    bottom = false,
    right = true,
  },
})

local cmp = require 'cmp'
local luasnip = require 'luasnip'

cmp.setup {
  snippet = {
    expand = function(args) luasnip.lsp_expand(args.body) end,
  },
  mapping = cmp.mapping.preset.insert {
    ['<C-d>'] = cmp.mapping.scroll_docs(-4),
    ['<C-f>'] = cmp.mapping.scroll_docs(4),
    ['<C-Space>'] = cmp.mapping.complete(),
    ['<CR>'] = cmp.mapping.confirm {
      behavior = cmp.ConfirmBehavior.Replace,
      select = true,
    },
    ['<Tab>'] = cmp.mapping(function(fallback)
      if cmp.visible() then
        cmp.select_next_item()
      elseif luasnip.expand_or_jumpable() then
        luasnip.expand_or_jump()
      else
        fallback()
      end
    end, { 'i', 's' }),
    ['<S-Tab>'] = cmp.mapping(function(fallback)
      if cmp.visible() then
        cmp.select_prev_item()
      elseif luasnip.jumpable(-1) then
        luasnip.jump(-1)
      else
        fallback()
      end
    end, { 'i', 's' }),
  },
  sources = {
    { name = 'nvim_lsp' },
    { name = 'path' },
    { name = 'buffer' },
    { name = 'luasnip' },
    { name = 'crates' },
  },
}

require('crates').setup {
  text = {
    loading = '  Loading...',
    version = '  %s',
    prerelease = '  %s',
    yanked = '  %s yanked',
    nomatch = '  Not found',
    upgrade = '  %s',
    error = '  Error fetching crate',
  },
  popup = {
    text = {
      title = '# %s',
      pill_left = '',
      pill_right = '',
      created_label = 'created        ',
      updated_label = 'updated        ',
      downloads_label = 'downloads      ',
      homepage_label = 'homepage       ',
      repository_label = 'repository     ',
      documentation_label = 'documentation  ',
      crates_io_label = 'crates.io      ',
      categories_label = 'categories     ',
      keywords_label = 'keywords       ',
      version = '%s',
      prerelease = '%s pre-release',
      yanked = '%s yanked',
      enabled = '* s',
      transitive = '~ s',
      normal_dependencies_title = '  Dependencies',
      build_dependencies_title = '  Build dependencies',
      dev_dependencies_title = '  Dev dependencies',
      optional = '? %s',
      loading = ' ...',
    },
  },
  src = {
    text = {
      prerelease = ' pre-release ',
      yanked = ' yanked ',
    },
  },
}

require('rust-tools').setup({
  tools = {
    inlay_hints = {
      enable = true,
      parameter_hints_prefix = '',
      other_hints_prefix = '',
    }
  },
  server = {
    on_attach = lsp_on_attach,
    settings = lsp_servers['rust_analyzer']
  }
})

require('auto-save').setup({})

require('nvim-tree').setup({
  renderer = {
    icons = {
      show = {
        file = false,
        folder = false,
        folder_arrow = false,
        git = true,
        modified = true
      }
    }
  }
})

require('nvim-autopairs').setup({})

require('telescope').setup {
  defaults = {
    layout_strategy = 'center',
  },
  pickers = {
    find_files = {
      find_command = { 'rg', '--files', '--hidden', '--glob', '!**/.git/*' },
    }
  }
}

require('spectre').setup({
  is_insert_mode = true
})
