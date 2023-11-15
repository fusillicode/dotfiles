---@diagnostic disable: missing-fields

local vm = vim

vm.g.mapleader = ' '
vm.g.maplocalleader = ' '

local lazypath = vm.fn.stdpath('data') .. '/lazy/lazy.nvim'
if not vm.loop.fs_stat(lazypath) then
  vm.fn.system({
    'git',
    'clone',
    '--filter=blob:none',
    'https://github.com/folke/lazy.nvm.git',
    '--branch=stable',
    lazypath,
  })
end
vm.opt.rtp:prepend(lazypath)

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
    'folke/tokyonight.nvim',
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
        cond = function() return vm.fn.executable 'make' == 1 end,
      },
    }
  },
  {
    'nvim-telescope/telescope-file-browser.nvim',
    dependencies = { 'nvim-telescope/telescope.nvim', 'nvim-lua/plenary.nvim' }
  },
  {
    'folke/todo-comments.nvim',
    dependencies = { 'nvim-lua/plenary.nvim' },
    opts = { signs = false, highlight = { after = '' } }
  },
  { 'saecki/crates.nvim',   dependencies = { 'nvim-lua/plenary.nvim' } },
  { 'ruifm/gitlinker.nvim', dependencies = { 'nvim-lua/plenary.nvim' } },
  'nvim-lualine/lualine.nvim',
  'lewis6991/gitsigns.nvim',
  'numToStr/Comment.nvim',
  'simrat39/rust-tools.nvim',
  'bogado/file-line',
  'windwp/nvim-autopairs',
  'andymass/vim-matchup',
  'mg979/vim-visual-multi',
  'mfussenegger/nvim-lint',
  'mhartington/formatter.nvim'
}

require 'tokyonight'.setup {
  styles = {
    comments = { italic = false, fg = 'grey' },
    functions = { bold = true },
    keywords = { bold = true, italic = false },
    types = { bold = true },
  },
  on_highlights = function(highlights, _)
    highlights.CursorLine = { bg = '#16161e' }
    highlights.CursorLineNr = { fg = 'white', bold = true }
    highlights.GitSignsAdd = { fg = 'limegreen' }
    highlights.GitSignsChange = { fg = 'orange' }
    highlights.GitSignsDelete = { fg = 'red' }
    highlights.LineNr = { fg = 'grey' }
    highlights.LspInlayHint = { fg = '#565f89' }
    highlights.MatchParen = { fg = 'black', bg = 'orange' }
  end,
  dim_inactive = true,
}
vm.cmd [[colorscheme tokyonight-night]]

vm.api.nvim_create_augroup('LspFormatOnSave', {})
vm.api.nvim_create_autocmd('BufWritePre', {
  group = 'LspFormatOnSave',
  callback = function() vm.lsp.buf.format({ async = false }) end,
})

vm.api.nvim_create_augroup('YankHighlight', { clear = true })
vm.api.nvim_create_autocmd('TextYankPost', {
  group = 'YankHighlight',
  pattern = '*',
  callback = function() vm.highlight.on_yank() end,
})

vm.api.nvim_create_autocmd('CursorHold', {
  callback = function()
    vm.diagnostic.open_float(nil, {
      focusable = false,
      close_events = { 'BufLeave', 'CursorMoved', 'InsertEnter', 'FocusLost' },
      source = 'always',
      scope = 'cursor',
    })
  end
})

vm.g.VM_theme = 'purplegray'

vm.o.autoindent = true
vm.o.backspace = 'indent,eol,start'
vm.o.breakindent = true
vm.o.colorcolumn = '120'
vm.o.completeopt = 'menuone,noselect'
vm.o.cursorline = true
vm.o.expandtab = true
vm.o.hlsearch = true
vm.o.ignorecase = true
vm.o.list = true
vm.o.mouse = 'a'
vm.o.number = true
vm.o.shiftwidth = 2
vm.o.sidescroll = 1
vm.o.signcolumn = 'yes'
vm.o.smartcase = true
vm.o.splitbelow = true
vm.o.splitright = true
vm.o.tabstop = 2
vm.o.termguicolors = true
vm.o.undofile = true
vm.o.updatetime = 250
vm.o.wrap = false
vm.opt.clipboard:append('unnamedplus')
vm.opt.iskeyword:append('-')
vm.wo.number = true
vm.wo.signcolumn = 'yes'

vm.keymap.set('', 'gn', ':bnext<CR>')
vm.keymap.set('', 'gp', ':bprevious<CR>')
vm.keymap.set('', 'ga', '<C-^>')
vm.keymap.set({ 'n', 'v' }, 'gh', '0')
vm.keymap.set({ 'n', 'v' }, 'gl', '$')
vm.keymap.set({ 'n', 'v' }, 'gs', '_')
vm.keymap.set({ 'n', 'v' }, 'mm', '%', { remap = true })
vm.keymap.set({ 'n', 'v' }, 'U', '<C-r>')
vm.keymap.set('v', '>', '>gv')
vm.keymap.set('v', '<', '<gv')
vm.keymap.set('n', '>', '>>')
vm.keymap.set('n', '<', '<<')
vm.keymap.set('n', '<C-u>', '<C-u>zz')
vm.keymap.set('n', '<C-d>', '<C-d>zz')
vm.keymap.set('n', '<C-j>', '<C-Down>', { remap = true })
vm.keymap.set('n', '<C-k>', '<C-Up>', { remap = true })
vm.keymap.set('n', 'dp', vm.diagnostic.goto_prev)
vm.keymap.set('n', 'dn', vm.diagnostic.goto_next)
vm.keymap.set('n', '<esc>', ':noh<CR>')
vm.keymap.set('n', '<C-s>', ':update<CR>')
vm.keymap.set('', '<C-c>', '<C-c>:noh<CR>')
vm.keymap.set('', '<C-r>', ':LspRestart<CR>')

vm.keymap.set({ 'n', 'v' }, '<leader><leader>', ':w! <CR>')
vm.keymap.set({ 'n', 'v' }, '<leader>x', ':bd <CR>')
vm.keymap.set({ 'n', 'v' }, '<leader>X', ':bd! <CR>')
vm.keymap.set({ 'n', 'v' }, '<leader>q', ':q <CR>')
vm.keymap.set({ 'n', 'v' }, '<leader>Q', ':q! <CR>')
vm.keymap.set({ 'n', 'v' }, '<leader>', '<Nop>')
vm.keymap.set('n', '<leader>b', require 'telescope.builtin'.buffers)
vm.keymap.set('n', '<leader>f', require 'telescope.builtin'.find_files)
vm.keymap.set('n', '<leader>F', ':Telescope file_browser path=%:p:h select_buffer=true<CR>')
vm.keymap.set('n', '<leader>l', require 'telescope'.extensions.live_grep_args.live_grep_args)
vm.keymap.set('n', '<leader>c', require 'telescope.builtin'.git_commits)
vm.keymap.set('n', '<leader>bc', require 'telescope.builtin'.git_bcommits)
vm.keymap.set('n', '<leader>gb', require 'telescope.builtin'.git_branches)
vm.keymap.set('n', '<leader>gs', require 'telescope.builtin'.git_status)
vm.keymap.set('n', '<leader>d', function() require 'telescope.builtin'.diagnostics({ bufnr = 0 }) end)
vm.keymap.set('n', '<leader>D', require 'telescope.builtin'.diagnostics)
vm.keymap.set('n', '<leader>s', require 'telescope.builtin'.lsp_document_symbols)
vm.keymap.set('n', '<leader>S', require 'telescope.builtin'.lsp_dynamic_workspace_symbols)
vm.keymap.set('n', '<leader>t', ':TodoTelescope<CR>')
vm.keymap.set('n', '<leader>z', vm.diagnostic.open_float)

local lsp_keybindings = function(_, bufnr)
  vm.keymap.set('n', 'gd', require 'telescope.builtin'.lsp_definitions, { buffer = bufnr })
  vm.keymap.set('n', 'gr', require 'telescope.builtin'.lsp_references, { buffer = bufnr })
  vm.keymap.set('n', 'gi', require 'telescope.builtin'.lsp_implementations, { buffer = bufnr })
  vm.keymap.set('n', 'K', vm.lsp.buf.hover, { buffer = bufnr })
  vm.keymap.set('n', '<leader>r', vm.lsp.buf.rename, { buffer = bufnr })
  vm.keymap.set('n', '<leader>a', vm.lsp.buf.code_action, { buffer = bufnr })
end

vm.lsp.handlers['textDocument/hover'] = vm.lsp.with(vm.lsp.handlers.hover, { border = 'single' })

vm.diagnostic.config {
  virtual_text = true,
  signs = true,
  update_in_insert = true,
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

require 'gitsigns'.setup {
  on_attach = function(_)
    local gs = package.loaded.gitsigns

    vm.keymap.set('n', ']c', function()
      if vm.wo.diff then return ']c' end
      vm.schedule(function() gs.next_hunk() end)
      return '<Ignore>'
    end, { expr = true })

    vm.keymap.set('n', '[c', function()
      if vm.wo.diff then return '[c' end
      vm.schedule(function() gs.prev_hunk() end)
      return '<Ignore>'
    end, { expr = true })

    vm.keymap.set('n', '<leader>hs', gs.stage_hunk)
    vm.keymap.set('n', '<leader>hr', gs.reset_hunk)
    vm.keymap.set('v', '<leader>hs', function() gs.stage_hunk { vm.fn.line('.'), vm.fn.line('v') } end)
    vm.keymap.set('v', '<leader>hr', function() gs.reset_hunk { vm.fn.line('.'), vm.fn.line('v') } end)
    vm.keymap.set('n', '<leader>hu', gs.undo_stage_hunk)
    vm.keymap.set('n', '<leader>tb', gs.toggle_current_line_blame)
    vm.keymap.set('n', '<leader>td', gs.toggle_deleted)
  end
}

require 'nvim-treesitter.configs'.setup {
  matchup = { enable = true, enable_quotes = true },
  ensure_installed = {
    'bash',
    'comment',
    'css',
    'diff',
    'dockerfile',
    'elm',
    'html',
    'javascript',
    'json',
    'kdl',
    'lua',
    'make',
    'markdown',
    'python',
    'regex',
    'rust',
    'sql',
    'textproto',
    'toml',
    'typescript',
    'xml',
    'yaml',
  },
  sync_install = true,
  auto_install = false,
  highlight = { enable = true },
  textobjects = {
    move = {
      enable = true,
      set_jumps = true,
      goto_next_start = {
        ['<C-l>'] = '@block.outer',
      },
      goto_previous_end = {
        ['<C-h>'] = '@block.outer',
      },
    },
  },
}

local lsp_servers = {
  bashls = {},
  docker_compose_language_service = {},
  dockerls = {},
  dotls = {},
  graphql = {},
  html = {},
  helm_ls = {},
  jsonls = {},
  lua_ls = {
    Lua = {
      workspace = { checkThirdParty = false },
      telemetry = { enable = false },
      diagnostics = {
        globals = { 'vim' }
      }
    },
  },
  marksman = {},
  ruff_lsp = {},
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
        extraArgs = { '--profile', 'rust-analyzer' },
        extraEnv = { CARGO_PROFILE_RUST_ANALYZER_INHERITS = 'dev' },
      }
    }
  },
  sqlls = {},
  taplo = {},
  tsserver = {},
  yamlls = {}
}

require 'neodev'.setup {}

require 'mason'.setup {}

local mason_lspconfig = require 'mason-lspconfig'
mason_lspconfig.setup {
  ensure_installed = vm.tbl_keys(lsp_servers),
}

local capabilities = vm.lsp.protocol.make_client_capabilities()
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
  progress = {
    display = {
      render_limit = 1,
      done_ttl = 1
    }
  }
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
    ['<C-u>'] = cmp.mapping.scroll_docs(4),
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
      highlight = 'LspInlayHint'
    }
  },
  server = {
    on_attach = lsp_keybindings,
    settings = lsp_servers['rust_analyzer']
  }
}


require 'nvim-autopairs'.setup {}

local fb_actions = require 'telescope._extensions.file_browser.actions'
require 'telescope'.setup {
  extensions = {
    file_browser = {
      hide_parent_dir = true,
      dir_icon = '',
      hidden = { file_browser = true, folder_browser = true },
      grouped = true,
      mappings = {
        ['n'] = {
          ['h'] = fb_actions.goto_parent_dir,
        }
      }
    }
  },
  pickers = {
    find_files = {
      find_command = { 'rg', '--files', '--hidden', '--glob', '!**/.git/*' },
    }
  }
}
require 'telescope'.load_extension 'fzf'
require 'telescope'.load_extension 'file_browser'

require 'gitlinker'.setup {}
