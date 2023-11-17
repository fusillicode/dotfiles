vim.g.mapleader = ' '
vim.g.maplocalleader = ' '

local lazypath = vim.fn.stdpath('data') .. '/lazy/lazy.nvim'
if not vim.loop.fs_stat(lazypath) then
  vim.fn.system({
    'git',
    'clone',
    '--filter=blob:none',
    'https://github.com/folke/lazy.nvm.git',
    '--branch=stable',
    lazypath,
  })
end
vim.opt.rtp:prepend(lazypath)

require('lazy').setup({
  {
    'neovim/nvim-lspconfig',
    dependencies = {
      'williamboman/mason.nvim',
      'williamboman/mason-lspconfig.nvim',
      {
        'j-hui/fidget.nvim',
        opts = {
          progress = {
            display = {
              render_limit = 1,
              done_ttl = 1
            }
          }
        }
      },
    },
    config = function()
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

      require('mason').setup({})

      local mason_lspconfig = require('mason-lspconfig')
      mason_lspconfig.setup({ ensure_installed = vim.tbl_keys(lsp_servers) })

      local capabilities = vim.lsp.protocol.make_client_capabilities()
      capabilities = require('cmp_nvim_lsp').default_capabilities(capabilities)

      local lsp_keybindings = function(_, bufnr)
        vim.keymap.set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr })
        vim.keymap.set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr })
        vim.keymap.set('n', '<leader>a', vim.lsp.buf.code_action, { buffer = bufnr })
      end

      local lspconfig = require('lspconfig')

      mason_lspconfig.setup_handlers {
        function(server_name)
          lspconfig[server_name].setup({
            capabilities = capabilities,
            on_attach = lsp_keybindings,
            settings = lsp_servers[server_name],
          })
        end,
      }
    end
  },
  {
    'hrsh7th/nvim-cmp',
    dependencies = {
      'L3MON4D3/LuaSnip',
      'hrsh7th/cmp-buffer',
      'hrsh7th/cmp-nvim-lsp',
      'hrsh7th/cmp-path',
      'rafamadriz/friendly-snippets',
      'saadparwaiz1/cmp_luasnip',
    },
    config = function()
      local cmp = require('cmp')
      local luasnip = require('luasnip')
      require("luasnip.loaders.from_vscode").lazy_load()

      cmp.setup({
        window = {
          completion = { border = 'single' },
          documentation = { border = 'single' },
        },
        snippet = {
          expand = function(args) luasnip.lsp_expand(args.body) end,
        },
        mapping = cmp.mapping.preset.insert({
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
        }),
        sources = {
          { name = 'nvim_lsp' },
          { name = 'path' },
          { name = 'buffer' },
          { name = 'luasnip' },
          { name = 'crates' },
        },
      })
    end
  },
  {
    'nvim-treesitter/nvim-treesitter',
    dependencies = { 'nvim-treesitter/nvim-treesitter-textobjects' },
    build = ':TSUpdate',
    config = function()
      require('nvim-treesitter.configs').setup({
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
      })
    end
  },
  {
    'rebelot/kanagawa.nvim',
    config = function()
      require('kanagawa').setup({
        compile = true,
        commentStyle = { italic = false },
        functionStyle = { bold = true },
        keywordStyle = { italic = false, bold = true },
        statementStyle = { bold = true },
        typeStyle = { bold = true },
        dimInactive = true,
        colors = {
          theme = { all = { ui = { bg_gutter = 'none' } } },
        },
        background = {
          dark = 'wave',
          light = 'lotus'
        },
        overrides = function(colors)
          local theme = colors.theme
          return {
            CursorLineNr = { fg = 'white', bold = true },
            GitSignsAdd = { fg = 'limegreen' },
            GitSignsChange = { fg = 'orange' },
            GitSignsDelete = { fg = 'red' },
            LineNr = { fg = 'grey' },
            LspInlayHint = { fg = 'grey' },
            MatchParen = { fg = 'black', bg = 'orange' },
            Pmenu = { fg = theme.ui.shade0, bg = theme.ui.bg_p1 },
            PmenuSel = { fg = "none", bg = theme.ui.bg_p2 },
            PmenuSbar = { bg = theme.ui.bg_m1 },
            PmenuThumb = { bg = theme.ui.bg_p2 },
          }
        end,
      })
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
        cond = function() return vim.fn.executable 'make' == 1 end,
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

        vim.keymap.set('n', ']c', function()
          if vim.wo.diff then return ']c' end
          vim.schedule(function() gs.next_hunk() end)
          return '<Ignore>'
        end, { expr = true })

        vim.keymap.set('n', '[c', function()
          if vim.wo.diff then return '[c' end
          vim.schedule(function() gs.prev_hunk() end)
          return '<Ignore>'
        end, { expr = true })

        vim.keymap.set('n', '<leader>hs', gs.stage_hunk)
        vim.keymap.set('n', '<leader>hr', gs.reset_hunk)
        vim.keymap.set('v', '<leader>hs', function() gs.stage_hunk({ vim.fn.line('.'), vim.fn.line('v') }) end)
        vim.keymap.set('v', '<leader>hr', function() gs.reset_hunk({ vim.fn.line('.'), vim.fn.line('v') }) end)
        vim.keymap.set('n', '<leader>hu', gs.undo_stage_hunk)
        vim.keymap.set('n', '<leader>tb', gs.toggle_current_line_blame)
        vim.keymap.set('n', '<leader>td', gs.toggle_deleted)
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
      vim.g.VM_theme = 'purplegray'
    end
  },
  'bogado/file-line',
  'andymass/vim-matchup',
  'mfussenegger/nvim-lint',
  'mhartington/formatter.nvim'
})

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

vim.keymap.set('', 'gn', ':bn<CR>')
vim.keymap.set('', 'gp', ':bp<CR>')
vim.keymap.set('', 'ga', '<C-^>')
vim.keymap.set({ 'n', 'v' }, 'gh', '0')
vim.keymap.set({ 'n', 'v' }, 'gl', '$')
vim.keymap.set({ 'n', 'v' }, 'gs', '_')
vim.keymap.set({ 'n', 'v' }, 'mm', '%', { remap = true })
vim.keymap.set({ 'n', 'v' }, 'U', '<C-r>')
vim.keymap.set('v', '>', '>gv')
vim.keymap.set('v', '<', '<gv')
vim.keymap.set('n', '>', '>>')
vim.keymap.set('n', '<', '<<')
vim.keymap.set('n', '<C-u>', '<C-u>zz')
vim.keymap.set('n', '<C-d>', '<C-d>zz')
vim.keymap.set('n', '<C-o>', '<C-o>zz')
vim.keymap.set('n', '<C-i>', '<C-i>zz')
vim.keymap.set('n', '<C-j>', '<C-Down>', { remap = true })
vim.keymap.set('n', '<C-k>', '<C-Up>', { remap = true })
vim.keymap.set('n', 'dp', vim.diagnostic.goto_prev)
vim.keymap.set('n', 'dn', vim.diagnostic.goto_next)
vim.keymap.set('n', '<leader>e', vim.diagnostic.open_float)
vim.keymap.set('n', '<esc>', ':noh<CR>')
vim.keymap.set('', '<C-r>', ':LspRestart<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader><leader>', ':w!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>x', ':bd<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>X', ':bd!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>w', ':wa<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>W', ':wa!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>q', ':q<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>Q', ':q!<CR>')
vim.keymap.set({ 'n', 'v' }, '<leader>', '<Nop>')

local telescope = require('telescope')
local telescope_builtin = require('telescope.builtin')
vim.keymap.set('n', '<leader>b', telescope_builtin.buffers)
vim.keymap.set('n', '<leader>f', telescope_builtin.find_files)
vim.keymap.set('n', '<leader>F', ':Telescope file_browser path=%:p:h select_buffer=true<CR>')
vim.keymap.set('n', '<leader>l', telescope.extensions.live_grep_args.live_grep_args)
vim.keymap.set('n', '<leader>c', telescope_builtin.git_commits)
vim.keymap.set('n', '<leader>bc', telescope_builtin.git_bcommits)
vim.keymap.set('n', '<leader>gb', telescope_builtin.git_branches)
vim.keymap.set('n', '<leader>gs', telescope_builtin.git_status)
vim.keymap.set('n', '<leader>d', function() telescope_builtin.diagnostics({ bufnr = 0 }) end)
vim.keymap.set('n', '<leader>D', telescope_builtin.diagnostics)
vim.keymap.set('n', '<leader>s', telescope_builtin.lsp_document_symbols)
vim.keymap.set('n', '<leader>S', telescope_builtin.lsp_dynamic_workspace_symbols)
vim.keymap.set('n', 'gd', telescope_builtin.lsp_definitions)
vim.keymap.set('n', 'gr', telescope_builtin.lsp_references)
vim.keymap.set('n', 'gi', telescope_builtin.lsp_implementations)
vim.keymap.set('n', '<leader>t', ':TodoTelescope<CR>')

vim.lsp.handlers['textDocument/hover'] = vim.lsp.with(vim.lsp.handlers.hover, { border = 'single' })

vim.diagnostic.config {
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


local fb_actions = require('telescope._extensions.file_browser.actions')
telescope.setup({
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
})
telescope.load_extension('fzf')
telescope.load_extension('file_browser')
