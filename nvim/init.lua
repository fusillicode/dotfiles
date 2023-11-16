---@diagnostic disable: missing-fields

local v = vim

v.g.mapleader = ' '
v.g.maplocalleader = ' '

local lazypath = v.fn.stdpath('data') .. '/lazy/lazy.nvim'
if not v.loop.fs_stat(lazypath) then
  v.fn.system({
    'git',
    'clone',
    '--filter=blob:none',
    'https://github.com/folke/lazy.nvm.git',
    '--branch=stable',
    lazypath,
  })
end
v.opt.rtp:prepend(lazypath)

require 'lazy'.setup({
  {
    'neovim/nvim-lspconfig',
    dependencies = {
      'williamboman/mason.nvim',
      'williamboman/mason-lspconfig.nvim',
      'j-hui/fidget.nvim',
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
    config = function()
      require('tokyonight').setup({
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
      })

      v.cmd([[colorscheme tokyonight-night]])
    end,
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
        cond = function() return v.fn.executable 'make' == 1 end,
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
  { 'saecki/crates.nvim',   dependencies = { 'nvim-lua/plenary.nvim' }, config = true },
  { 'ruifm/gitlinker.nvim', dependencies = { 'nvim-lua/plenary.nvim' }, config = true },
  {
    'nvim-lualine/lualine.nvim',
    opts = {
      options = {
        component_separators = '',
        icons_enabled = false,
        section_separators = '',
        theme = 'auto',
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
  },
  {
    'lewis6991/gitsigns.nvim',
    opts = {
      on_attach = function(_)
        local gs = package.loaded.gitsigns

        v.keymap.set('n', ']c', function()
          if v.wo.diff then return ']c' end
          v.schedule(function() gs.next_hunk() end)
          return '<Ignore>'
        end, { expr = true })

        v.keymap.set('n', '[c', function()
          if v.wo.diff then return '[c' end
          v.schedule(function() gs.prev_hunk() end)
          return '<Ignore>'
        end, { expr = true })

        v.keymap.set('n', '<leader>hs', gs.stage_hunk)
        v.keymap.set('n', '<leader>hr', gs.reset_hunk)
        v.keymap.set('v', '<leader>hs', function() gs.stage_hunk { v.fn.line('.'), v.fn.line('v') } end)
        v.keymap.set('v', '<leader>hr', function() gs.reset_hunk { v.fn.line('.'), v.fn.line('v') } end)
        v.keymap.set('n', '<leader>hu', gs.undo_stage_hunk)
        v.keymap.set('n', '<leader>tb', gs.toggle_current_line_blame)
        v.keymap.set('n', '<leader>td', gs.toggle_deleted)
      end
    }
  },
  {
    'numToStr/Comment.nvim',
    opts = { opleader = { line = '<C-c>' }, toggler = { line = '<C-c>' } }
  },
  {
    'lvimuser/lsp-inlayhints.nvim',
    event = 'LspAttach',
    config = function()
      vim.api.nvim_create_augroup("LspAttach_inlayhints", {})
      vim.api.nvim_create_autocmd("LspAttach", {
        group = "LspAttach_inlayhints",
        callback = function(args)
          if not (args.data and args.data.client_id) then
            return
          end

          local client = vim.lsp.get_client_by_id(args.data.client_id)
          require("lsp-inlayhints").on_attach(client, args.buf)
        end,
      })

      require('lsp-inlayhints').setup({
        inlay_hints = {
          parameter_hints = {
            prefix = '',
          }
        }
      })
    end,
  },
  { 'windwp/nvim-autopairs', config = true },
  {
    'mg979/vim-visual-multi',
    config = function()
      v.g.VM_theme = 'purplegray'
    end
  },
  'bogado/file-line',
  'andymass/vim-matchup',
  'mfussenegger/nvim-lint',
  'mhartington/formatter.nvim'
}, {
  ui = { border = 'single' }
})

v.api.nvim_create_augroup('LspFormatOnSave', {})
v.api.nvim_create_autocmd('BufWritePre', {
  group = 'LspFormatOnSave',
  callback = function() v.lsp.buf.format({ async = false }) end,
})

v.api.nvim_create_augroup('YankHighlight', { clear = true })
v.api.nvim_create_autocmd('TextYankPost', {
  group = 'YankHighlight',
  pattern = '*',
  callback = function() v.highlight.on_yank() end,
})

v.api.nvim_create_autocmd('CursorHold', {
  callback = function()
    v.diagnostic.open_float(nil, {
      focusable = false,
      close_events = { 'BufLeave', 'CursorMoved', 'InsertEnter', 'FocusLost' },
      source = 'always',
      scope = 'cursor',
    })
  end
})

v.o.autoindent = true
v.o.backspace = 'indent,eol,start'
v.o.breakindent = true
v.o.colorcolumn = '120'
v.o.completeopt = 'menuone,noselect'
v.o.cursorline = true
v.o.expandtab = true
v.o.hlsearch = true
v.o.ignorecase = true
v.o.list = true
v.o.mouse = 'a'
v.o.number = true
v.o.shiftwidth = 2
v.o.sidescroll = 1
v.o.signcolumn = 'yes'
v.o.smartcase = true
v.o.splitbelow = true
v.o.splitright = true
v.o.tabstop = 2
v.o.termguicolors = true
v.o.undofile = true
v.o.updatetime = 250
v.o.wrap = false
v.opt.clipboard:append('unnamedplus')
v.opt.iskeyword:append('-')
v.wo.number = true
v.wo.signcolumn = 'yes'

v.keymap.set('', 'gn', ':bnext<CR>')
v.keymap.set('', 'gp', ':bprev<CR>')
v.keymap.set('', 'ga', '<C-^>')
v.keymap.set({ 'n', 'v' }, 'gh', '0')
v.keymap.set({ 'n', 'v' }, 'gl', '$')
v.keymap.set({ 'n', 'v' }, 'gs', '_')
v.keymap.set({ 'n', 'v' }, 'mm', '%', { remap = true })
v.keymap.set({ 'n', 'v' }, 'U', '<C-r>')
v.keymap.set('v', '>', '>gv')
v.keymap.set('v', '<', '<gv')
v.keymap.set('n', '>', '>>')
v.keymap.set('n', '<', '<<')
v.keymap.set('n', '<C-u>', '<C-u>zz')
v.keymap.set('n', '<C-u>', '<C-u>zz')
v.keymap.set('n', '<C-o>', '<C-o>zz')
v.keymap.set('n', '<C-i>', '<C-i>zz')
v.keymap.set('n', '<C-j>', '<C-Down>', { remap = true })
v.keymap.set('n', '<C-k>', '<C-Up>', { remap = true })
v.keymap.set('n', 'dp', v.diagnostic.goto_prev)
v.keymap.set('n', 'dn', v.diagnostic.goto_next)
v.keymap.set('n', '<esc>', ':noh<CR>')
v.keymap.set('n', '<C-s>', ':update<CR>')
v.keymap.set('', '<C-c>', '<C-c>:noh<CR>')
v.keymap.set('', '<C-r>', ':LspRestart<CR>')

v.keymap.set({ 'n', 'v' }, '<leader><leader>', ':w! <CR>')
v.keymap.set({ 'n', 'v' }, '<leader>x', ':bd <CR>')
v.keymap.set({ 'n', 'v' }, '<leader>X', ':bd! <CR>')
v.keymap.set({ 'n', 'v' }, '<leader>q', ':q <CR>')
v.keymap.set({ 'n', 'v' }, '<leader>Q', ':q! <CR>')
v.keymap.set({ 'n', 'v' }, '<leader>', '<Nop>')

local telescope = require 'telescope'
local telescope_builtin = require 'telescope.builtin'
v.keymap.set('n', '<leader>b', telescope_builtin.buffers)
v.keymap.set('n', '<leader>f', telescope_builtin.find_files)
v.keymap.set('n', '<leader>F', ':Telescope file_browser path=%:p:h select_buffer=true<CR>')
v.keymap.set('n', '<leader>l', telescope.extensions.live_grep_args.live_grep_args)
v.keymap.set('n', '<leader>c', telescope_builtin.git_commits)
v.keymap.set('n', '<leader>bc', telescope_builtin.git_bcommits)
v.keymap.set('n', '<leader>gb', telescope_builtin.git_branches)
v.keymap.set('n', '<leader>gs', telescope_builtin.git_status)
v.keymap.set('n', '<leader>d', function() telescope_builtin.diagnostics({ bufnr = 0 }) end)
v.keymap.set('n', '<leader>D', telescope_builtin.diagnostics)
v.keymap.set('n', '<leader>s', telescope_builtin.lsp_document_symbols)
v.keymap.set('n', '<leader>S', telescope_builtin.lsp_dynamic_workspace_symbols)
v.keymap.set('n', '<leader>t', ':TodoTelescope<CR>')
v.keymap.set('n', '<leader>z', v.diagnostic.open_float)

local lsp_keybindings = function(_, bufnr)
  v.keymap.set('n', 'gd', telescope_builtin.lsp_definitions, { buffer = bufnr })
  v.keymap.set('n', 'gr', telescope_builtin.lsp_references, { buffer = bufnr })
  v.keymap.set('n', 'gi', telescope_builtin.lsp_implementations, { buffer = bufnr })
  v.keymap.set('n', 'K', v.lsp.buf.hover, { buffer = bufnr })
  v.keymap.set('n', '<leader>r', v.lsp.buf.rename, { buffer = bufnr })
  v.keymap.set('n', '<leader>a', v.lsp.buf.code_action, { buffer = bufnr })
end

v.lsp.handlers['textDocument/hover'] = v.lsp.with(v.lsp.handlers.hover, { border = 'single' })

v.diagnostic.config {
  float = {
    border = 'single',
    focusable = false,
    header = '',
    prefix = '',
    source = 'always',
    style = 'minimal',
  },
  severity_sort = true,
  signs = true,
  underline = false,
  update_in_insert = true,
  virtual_text = true
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
      diagnostics = { globals = { 'vim' } },
      telemetry = { enable = false },
      workspace = { checkThirdParty = false }
    },
  },
  marksman = {},
  ruff_lsp = {},
  rust_analyzer = {
    ['rust-analyzer'] = {
      cargo = {
        build_script = { enable = true },
        extraArgs = { '--profile', 'rust-analyzer' },
        extraEnv = { CARGO_PROFILE_RUST_ANALYZER_INHERITS = 'dev' },
      },
      check = { command = 'clippy' },
      checkOnSave = { command = 'clippy' },
      completion = { autoimport = { enable = true } },
      imports = { enforce = true, granularity = { group = 'item' }, prefix = 'crate' },
      lens = { debug = { enable = false }, implementations = { enable = false }, run = { enable = false } },
      proc_macro = { enable = true },
      showUnlinkedFileNotification = false
    }
  },
  sqlls = {},
  taplo = {},
  tsserver = {},
  yamlls = {}
}

require 'mason'.setup { ui = { border = 'single' } }

local capabilities = v.lsp.protocol.make_client_capabilities()
capabilities = require 'cmp_nvim_lsp'.default_capabilities(capabilities)

local mason_lspconfig = require 'mason-lspconfig'
mason_lspconfig.setup { ensure_installed = v.tbl_keys(lsp_servers) }

local lspconfig = require 'lspconfig'

mason_lspconfig.setup_handlers {
  function(server_name)
    lspconfig[server_name].setup({
      capabilities = capabilities,
      on_attach = lsp_keybindings,
      settings = lsp_servers[server_name],
    })
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

local fb_actions = require 'telescope._extensions.file_browser.actions'
telescope.setup {
  defaults = { layout_strategy = 'vertical' },
  extensions = {
    file_browser = {
      dir_icon = '',
      grouped = true,
      hidden = { file_browser = true, folder_browser = true },
      hide_parent_dir = true,
      hijack_netrw = true,
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
telescope.load_extension 'fzf'
telescope.load_extension 'file_browser'
