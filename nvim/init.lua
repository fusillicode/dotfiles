vim.g.mapleader = ' '
vim.g.maplocalleader = ' '

local lazypath = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
if not vim.loop.fs_stat(lazypath) then
  vim.fn.system({
    "git",
    "clone",
    "--filter=blob:none",
    "https://github.com/folke/lazy.nvim.git",
    "--branch=stable",
    lazypath,
  })
end
vim.opt.rtp:prepend(lazypath)

require 'lazy'.setup {
  {
    'neovim/nvim-lspconfig',
    dependencies = {
      'williamboman/mason.nvim',
      'williamboman/mason-lspconfig.nvim',
      'j-hui/fidget.nvim',
      'folke/neodev.nvim',
    },
  },
  {
    'hrsh7th/nvim-cmp',
    dependencies = {
      'hrsh7th/cmp-nvim-lsp',
      'hrsh7th/cmp-buffer',
      'hrsh7th/cmp-path',
      'saadparwaiz1/cmp_luasnip',
      'L3MON4D3/LuaSnip',
    },
  },
  {
    'nvim-treesitter/nvim-treesitter',
    dependencies = { 'nvim-treesitter/nvim-treesitter-textobjects' },
    build = ':TSUpdate',
  },
  {
    "folke/tokyonight.nvim",
    lazy = false,
    priority = 1000,
    opts = {},
  },
  {
    'nvim-telescope/telescope.nvim',
    branch = '0.1.x',
    dependencies = {
      'nvim-lua/plenary.nvim',
      'nvim-telescope/telescope-live-grep-args.nvim',
      {
        'nvim-telescope/telescope-fzf-native.nvim',
        build = 'make',
        cond = function() return vim.fn.executable 'make' == 1 end,
      },
    }
  },
  'nvim-lualine/lualine.nvim',
  'lewis6991/gitsigns.nvim',
  'numToStr/Comment.nvim',
  'simrat39/rust-tools.nvim',
  { 'saecki/crates.nvim', dependencies = { 'nvim-lua/plenary.nvim' } },
  'bogado/file-line',
  'windwp/nvim-autopairs',
  'gbprod/cutlass.nvim',
}

vim.cmd('autocmd BufWritePre * lua vim.lsp.buf.formatting_sync()')
vim.cmd('autocmd! CursorHold,CursorHoldI * lua vim.diagnostic.open_float(nil, { focus = false })')
vim.cmd.colorscheme 'tokyonight'

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
vim.o.sidescroll = 1
vim.o.signcolumn = 'yes'
vim.o.smartcase = true
vim.o.splitbelow = true
vim.o.splitright = true
vim.o.tabstop = 2
vim.o.termguicolors = true
vim.o.undofile = true
vim.o.updatetime = 250
vim.o.wrap = false
vim.opt.clipboard:append('unnamedplus')
vim.opt.iskeyword:append('-')
vim.wo.number = true
vim.wo.signcolumn = 'yes'

vim.keymap.set({ 'n', 'v' }, '<leader>', '<Nop>')
vim.keymap.set('v', '>', '>gv')
vim.keymap.set('v', '<', '<gv')
vim.keymap.set('n', '<esc>', ':noh <CR>', {})
vim.keymap.set('n', '<C-s>', ':update <CR>', {})
vim.keymap.set('', '<C-c>', '<C-c> :noh <CR>', {})
vim.keymap.set('n', '<leader>fd', ':bd! <CR>', {})
vim.keymap.set('n', '<leader>fo', require 'telescope.builtin'.oldfiles)
vim.keymap.set('n', '<leader>fb', require 'telescope.builtin'.buffers)
vim.keymap.set('n', '<leader>ff', require 'telescope.builtin'.find_files)
vim.keymap.set('n', '<leader>fw', require('telescope').extensions.live_grep_args.live_grep_args)
vim.keymap.set('n', '<leader>gc', require 'telescope.builtin'.git_commits)
vim.keymap.set('n', '<leader>gbc', require 'telescope.builtin'.git_bcommits)
vim.keymap.set('n', '<leader>gb', require 'telescope.builtin'.git_branches)
vim.keymap.set('n', '<leader>gs', require 'telescope.builtin'.git_status)
vim.keymap.set('n', '<leader>e', vim.diagnostic.open_float)
vim.keymap.set('n', '[d', vim.diagnostic.goto_prev)
vim.keymap.set('n', ']d', vim.diagnostic.goto_next)

local lsp_on_attach = function(_, bufnr)
  vim.keymap.set('n', 'gd', vim.lsp.buf.definition, { buffer = bufnr })
  vim.keymap.set('n', 'gD', vim.lsp.buf.declaration, { buffer = bufnr })
  vim.keymap.set('n', 'gr', require 'telescope.builtin'.lsp_references, { buffer = bufnr })
  vim.keymap.set('n', 'gi', vim.lsp.buf.implementation, { buffer = bufnr })
  vim.keymap.set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr })
  vim.keymap.set('n', '<C-k>', vim.lsp.buf.signature_help, { buffer = bufnr })
  vim.keymap.set('n', '<leader>rn', vim.lsp.buf.rename, { buffer = bufnr })
  vim.keymap.set('n', '<leader>ca', vim.lsp.buf.code_action, { buffer = bufnr })
  vim.api.nvim_buf_create_user_command(bufnr, 'Format', function(_) vim.lsp.buf.format() end, {})
end

vim.lsp.handlers["textDocument/hover"] = vim.lsp.with(vim.lsp.handlers.hover, { border = "single" })
vim.lsp.handlers["textDocument/signatureHelp"] = vim.lsp.with(
  vim.lsp.handlers.signature_help, { border = "single" }
)

vim.diagnostic.config {
  virtual_text = true,
  signs = true,
  update_in_insert = false,
  underline = false,
  severity_sort = true,
  float = {
    focusable = false,
    style = 'minimal',
    border = 'single',
    source = 'always',
    header = '',
    prefix = '',
  },
}

local highlight_group = vim.api.nvim_create_augroup('YankHighlight', { clear = true })
vim.api.nvim_create_autocmd('TextYankPost', {
  callback = function() vim.highlight.on_yank() end,
  group = highlight_group,
  pattern = '*',
})

require 'tokyonight'.setup {
  style = 'night',
  styles = {
    types = { bold = true },
    keywords = { bold = true },
    functions = { bold = true },
  },
  dim_inactive = true,
}

require 'lualine'.setup {
  options = {
    icons_enabled = false,
    theme = 'auto',
    component_separators = '',
    section_separators = '',
  },
  sections = {
    lualine_a = {},
    lualine_b = {},
    lualine_c = { 'diagnostics', 'diff', { 'filename', file_status = true, path = 1 }, 'encoding' },
    lualine_x = { { 'branch', fmt = function(str) return str:sub(1, 33) end } },
    lualine_y = {},
    lualine_z = {}
  },
}

require 'Comment'.setup {}

require 'gitsigns'.setup {}

require 'nvim-treesitter.configs'.setup {
  ensure_installed = { 'rust', 'lua', 'python' },
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
  lua_ls = {
    Lua = {
      workspace = { checkThirdParty = false },
      telemetry = { enable = false },
      diagnostics = {
        globals = { 'vim' }
      }
    },
  },
  pyright = {},
}

require 'neodev'.setup {}

require 'mason'.setup {}

local mason_lspconfig = require 'mason-lspconfig'
mason_lspconfig.setup {
  ensure_installed = vim.tbl_keys(lsp_servers),
}

local capabilities = vim.lsp.protocol.make_client_capabilities()
capabilities = require 'cmp_nvim_lsp'.default_capabilities(capabilities)

mason_lspconfig.setup_handlers {
  function(server_name)
    require 'lspconfig'[server_name].setup {
      capabilities = capabilities,
      on_attach = lsp_on_attach,
      settings = lsp_servers[server_name],
    }
  end,
}

require 'fidget'.setup {
  align = {
    bottom = false,
    right = true,
  },
}

local cmp = require 'cmp'
local luasnip = require 'luasnip'

cmp.setup {
  window = {
    completion = { border = 'single' },
    documentation = { border = 'single' },
  },
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

require 'crates'.setup {
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

require 'rust-tools'.setup {
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
}


require 'nvim-autopairs'.setup {}

require 'telescope'.setup {
  defaults = require 'telescope.themes'.get_ivy {
    layout_config = {
      height = 13
    }
  },
  pickers = {
    find_files = {
      theme = 'ivy',
      layout_config = {
        height = 13
      },
      find_command = { 'rg', '--files', '--hidden', '--glob', '!**/.git/*' },
    }
  }
}
pcall(require('telescope').load_extension, 'fzf')

require 'cutlass'.setup {}
