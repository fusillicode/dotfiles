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
  {
    "folke/todo-comments.nvim",
    dependencies = { "nvim-lua/plenary.nvim" },
    opts = { signs = false }
  },
  { 'saecki/crates.nvim',   dependencies = { 'nvim-lua/plenary.nvim' } },
  { 'ruifm/gitlinker.nvim', dependencies = 'nvim-lua/plenary.nvim' },
  'nvim-lualine/lualine.nvim',
  'lewis6991/gitsigns.nvim',
  'numToStr/Comment.nvim',
  'simrat39/rust-tools.nvim',
  'bogado/file-line',
  'windwp/nvim-autopairs',
  'andymass/vim-matchup',
}

require 'tokyonight'.setup {
  styles = {
    types = { bold = true },
    keywords = { bold = true },
    functions = { bold = true },
  },
  on_highlights = function(highlights, _)
    highlights.CursorLine = { bg = "#16161e" }
    highlights.CursorLineNr = { fg = "white", bold = true }
    highlights.MatchParen = { fg = "black", bg = "orange" }
    highlights.LineNr = { fg = "gray" }
  end,
  dim_inactive = true,
}
vim.cmd [[colorscheme tokyonight-night]]

vim.api.nvim_create_augroup('LspFormatOnSave', {})
vim.api.nvim_create_autocmd('BufWritePre', {
  group = 'LspFormatOnSave',
  callback = function() vim.lsp.buf.format({ async = false }) end,
})

vim.api.nvim_create_augroup('YankHighlight', { clear = true })
vim.api.nvim_create_autocmd('TextYankPost', {
  group = 'YankHighlight',
  pattern = '*',
  callback = function() vim.highlight.on_yank() end,
})

vim.api.nvim_create_autocmd("CursorHold", {
  buffer = bufnr,
  callback = function()
    vim.diagnostic.open_float(nil, {
      focusable = false,
      close_events = { "BufLeave", "CursorMoved", "InsertEnter", "FocusLost" },
      source = 'always',
      scope = 'cursor',
    })
  end
})

vim.o.autoindent = true
vim.o.backspace = 'indent,eol,start'
vim.o.breakindent = true
vim.o.colorcolumn = '120'
vim.o.completeopt = 'menuone,noselect'
vim.o.cursorline = true
vim.o.expandtab = true
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

vim.keymap.set('', 'gn', ':bnext<CR>', {})
vim.keymap.set('', 'gp', ':bprevious<CR>', {})
vim.keymap.set('', 'ga', '<C-^>', {})
vim.keymap.set({ 'n', 'v' }, 'gh', '0')
vim.keymap.set({ 'n', 'v' }, 'gl', '$')
vim.keymap.set({ 'n', 'v' }, 'gs', '_')
vim.keymap.set({ 'n', 'v' }, 'mm', '%', { remap = true })
vim.keymap.set({ 'n', 'v' }, 'U', '<C-r>')
vim.keymap.set({ 'n', 'v' }, '>', '>gv')
vim.keymap.set({ 'n', 'v' }, '<', '<gv')
vim.keymap.set('n', 'dp', vim.diagnostic.goto_prev)
vim.keymap.set('n', 'dn', vim.diagnostic.goto_next)
vim.keymap.set('n', '<esc>', ':noh<CR>', {})
vim.keymap.set('n', '<C-s>', ':update<CR>', {})
vim.keymap.set('', '<C-c>', '<C-c>:noh<CR>', {})
vim.keymap.set('', '<C-r>', ':LspRestart<CR>', {})

vim.keymap.set({ 'n', 'v' }, '<leader><leader>', ':w! <CR>', {})
vim.keymap.set({ 'n', 'v' }, '<leader>x', ':bd <CR>', {})
vim.keymap.set({ 'n', 'v' }, '<leader>X', ':bd! <CR>', {})
vim.keymap.set({ 'n', 'v' }, '<leader>q', ':q <CR>', {})
vim.keymap.set({ 'n', 'v' }, '<leader>Q', ':q! <CR>', {})
vim.keymap.set({ 'n', 'v' }, '<leader>', '<Nop>')
vim.keymap.set('n', '<leader>b', require 'telescope.builtin'.buffers)
vim.keymap.set('n', '<leader>f', require 'telescope.builtin'.find_files)
vim.keymap.set('n', '<leader>l', require 'telescope'.extensions.live_grep_args.live_grep_args)
vim.keymap.set('n', '<leader>c', require 'telescope.builtin'.git_commits)
vim.keymap.set('n', '<leader>bc', require 'telescope.builtin'.git_bcommits)
vim.keymap.set('n', '<leader>gb', require 'telescope.builtin'.git_branches)
vim.keymap.set('n', '<leader>s', require 'telescope.builtin'.git_status)
vim.keymap.set('n', '<leader>d', require 'telescope.builtin'.diagnostics, { buffer = 0 })
vim.keymap.set('n', '<leader>D', require 'telescope.builtin'.diagnostics)
vim.keymap.set('n', '<leader>s', require 'telescope.builtin'.lsp_document_symbols)
vim.keymap.set('n', '<leader>S', require 'telescope.builtin'.lsp_workspace_symbols)
vim.keymap.set('n', '<leader>t', ':TodoTelescope<CR>')
vim.keymap.set('n', '<leader>e', vim.diagnostic.open_float)

local lsp_keybindings = function(_, bufnr)
  vim.keymap.set('n', 'gd', vim.lsp.buf.definition, { buffer = bufnr })
  vim.keymap.set('n', 'gD', vim.lsp.buf.declaration, { buffer = bufnr })
  vim.keymap.set('n', 'gr', require 'telescope.builtin'.lsp_references, { buffer = bufnr })
  vim.keymap.set('n', 'gi', vim.lsp.buf.implementation, { buffer = bufnr })
  vim.keymap.set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr })
  vim.keymap.set('n', '<C-k>', vim.lsp.buf.signature_help, { buffer = bufnr })
  vim.keymap.set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr })
  vim.keymap.set('n', '<leader>a', vim.lsp.buf.code_action, { buffer = bufnr })
end

vim.lsp.handlers["textDocument/hover"] = vim.lsp.with(vim.lsp.handlers.hover, { border = "single" })
vim.lsp.handlers["textDocument/signatureHelp"] = vim.lsp.with(
  vim.lsp.handlers.signature_help, { border = "single" }
)

vim.diagnostic.config {
  virtual_text = false,
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
    lualine_c = { { 'diagnostics', sources = { 'nvim_diagnostic' } }, { 'filename', file_status = true, path = 1 } },
    lualine_x = { { 'diagnostics', sources = { 'nvim_workspace_diagnostic' } } },
    lualine_y = {},
    lualine_z = {}
  },
}

require 'Comment'.setup { toggler = { line = '<C-c>' }, opleader = { line = '<C-c>' } }

require 'gitsigns'.setup {}

require 'nvim-treesitter.configs'.setup {
  matchup = { enable = true, enable_quotes = true },
  ensure_installed = { 'rust', 'lua', 'python' },
  sync_install = true,
  auto_install = false,
  highlight = { enable = true },
  textobjects = {
    move = {
      enable = true,
      set_jumps = true,
      goto_next_start = {
        ['L'] = '@block.outer',
      },
      goto_previous_end = {
        ['H'] = '@block.outer',
      },
    },
  },
}

local lsp_servers = {
  rust_analyzer = {
    ['rust-analyzer'] = {
      check = { command = 'clippy' },
      checkOnSave = { command = 'clippy' },
      completion = {
        autoimport = { enable = true }
      },
      imports = {
        enforce = true,
        granularity = {
          group = 'item',
        },
        prefix = 'crate',
      },
      lens = {
        debug = { enable = false },
        implementations = { enable = false },
        run = { enable = false },
      },
      showUnlinkedFileNotification = false,
      cargo = {
        extraArgs = { "--profile", "rust-analyzer" },
        extraEnv = { CARGO_PROFILE_RUST_ANALYZER_INHERITS = "dev" },
      }
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
      on_attach = lsp_keybindings,
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

require 'crates'.setup {}

require 'rust-tools'.setup {
  tools = {
    inlay_hints = {
      enable = true,
      parameter_hints_prefix = '',
      other_hints_prefix = '',
    }
  },
  server = {
    on_attach = lsp_keybindings,
    settings = lsp_servers['rust_analyzer']
  }
}


require 'nvim-autopairs'.setup {}

require 'telescope'.setup {
  pickers = {
    find_files = {
      find_command = { 'rg', '--files', '--hidden', '--glob', '!**/.git/*' },
    }
  }
}
require 'telescope'.load_extension('fzf')

require 'gitlinker'.setup {}
