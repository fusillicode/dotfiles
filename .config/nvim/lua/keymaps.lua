local M = {}

local function keymap_set(modes, lhs, rhs, opts)
  vim.keymap.set(modes, lhs, rhs, vim.tbl_extend('force', { silent = true, }, opts or {}))
end

function M.core()
  vim.g.mapleader = ' '
  vim.g.maplocalleader = ' '

  keymap_set('', '<c-n>', ':cn<cr>')
  keymap_set('', '<c-p>', ':cp<cr>')
  keymap_set('', '<c-x>', ':ccl<cr>')
  -- https://stackoverflow.com/a/3003636
  keymap_set('n', 'i', function()
    if vim.fn.empty(vim.fn.getline('.')) == 1 then
      return '\"_cc'
    else
      return 'i'
    end
  end, { expr = true, })
  keymap_set('i', '<c-a>', '<esc>^i')
  keymap_set('n', '<c-a>', '^i')
  keymap_set('i', '<c-e>', '<end>')
  keymap_set('n', '<c-e>', '$a')

  keymap_set('', 'gn', ':bn<cr>')
  keymap_set('', 'gp', ':bp<cr>')
  keymap_set({ 'n', 'v', }, 'gh', '0')
  keymap_set({ 'n', 'v', }, 'gl', '$')
  keymap_set({ 'n', 'v', }, 'gs', '_')

  -- https://github.com/Abstract-IDE/abstract-autocmds/blob/main/lua/abstract-autocmds/mappings.lua#L8-L14
  keymap_set('n', 'dd', function()
    if vim.api.nvim_get_current_line():match('^%s*$') then
      return '"_dd'
    end
    return 'dd'
  end, { noremap = true, expr = true, })
  keymap_set({ 'n', 'v', }, 'x', '"_x')
  keymap_set({ 'n', 'v', }, 'x', '"_x')
  keymap_set({ 'n', 'v', }, 'X', '"_X')
  keymap_set({ 'n', 'v', }, '<leader>yf', ':let @+ = expand("%") . ":" . line(".")<cr>')
  keymap_set('v', 'y', 'ygv<esc>')
  keymap_set('v', 'p', '"_dP')

  keymap_set('v', '>', '>gv')
  keymap_set('v', '<', '<gv')
  keymap_set('n', '>', '>>')
  keymap_set('n', '<', '<<')
  keymap_set({ 'n', 'v', }, 'U', '<c-r>')

  keymap_set({ 'n', 'v', }, '<leader><leader>', ':silent :w!<cr>')
  keymap_set({ 'n', 'v', }, '<leader>x', ':bd<cr>')
  keymap_set({ 'n', 'v', }, '<leader>X', ':bd!<cr>')
  keymap_set({ 'n', 'v', }, '<leader>w', ':wa<cr>')
  keymap_set({ 'n', 'v', }, '<leader>W', ':wa!<cr>')
  keymap_set({ 'n', 'v', }, '<leader>q', ':q<cr>')
  keymap_set({ 'n', 'v', }, '<leader>Q', ':q!<cr>')

  keymap_set({ 'n', 'v', }, '<c-x>', ':set wrap!<cr>')
  keymap_set('n', '<esc>', require('utils').normal_esc)
  keymap_set('v', '<esc>', require('utils').visual_esc, { expr = true, })

  keymap_set('n', '<leader>e', vim.diagnostic.open_float)

  keymap_set('n', '<leader>gx', require('opener').open_under_cursor)
  keymap_set('v', '<leader>gx', require('opener').open_selection)
end

function M.telescope(telescope, telescope_builtin, defaults)
  local function with_defaults(picker, opts)
    return function()
      telescope_builtin[picker](vim.tbl_extend('force', defaults, opts or {}))
    end
  end

  keymap_set('n', 'gd', with_defaults('lsp_definitions', { prompt_prefix = 'LSP Defs: ', }))
  keymap_set('n', 'gr', with_defaults('lsp_references', { prompt_prefix = 'LSP Refs: ', }))
  keymap_set('n', 'gi', with_defaults('lsp_implementations', { prompt_prefix = 'LSP Impls: ', }))
  keymap_set('n', '<leader>s', with_defaults('lsp_document_symbols', { prompt_prefix = 'LSP Syms: ', }))
  keymap_set('n', '<leader>S', with_defaults('lsp_dynamic_workspace_symbols', { prompt_prefix = 'LSP Syms*: ', }))
  keymap_set('n', '<leader>b', with_defaults('buffers', { prompt_prefix = 'Bufs: ', }))
  keymap_set('n', '<leader>f', with_defaults('find_files', { prompt_prefix = 'Files: ', }))
  keymap_set('n', '<leader>j', with_defaults('jumplist', { prompt_prefix = 'Jumps: ', }))
  keymap_set('n', '<leader>gc', with_defaults('git_commits', { prompt_prefix = 'gc*: ', }))
  keymap_set('n', '<leader>gcb', with_defaults('git_bcommits', { prompt_prefix = 'gc: ', bufnr = 0, }))
  keymap_set('n', '<leader>gb', with_defaults('git_branches', { prompt_prefix = 'gb: ', }))
  keymap_set('n', '<leader>gs', with_defaults('git_status', { prompt_prefix = 'gst: ', }))
  keymap_set('n', '<leader>d', with_defaults('diagnostics', { prompt_prefix = 'Diags: ', bufnr = 0, }))
  keymap_set('n', '<leader>D', with_defaults('diagnostics', { prompt_prefix = 'Diags*: ', }))
  keymap_set('n', '<leader>h', with_defaults('help_tags', { prompt_prefix = 'Help: ', }))
  keymap_set('n', '<leader>c', with_defaults('commands', { prompt_prefix = 'Cmds: ', }))
  keymap_set('n', '<leader>/', function()
    telescope.extensions.live_grep_args.live_grep_args(vim.tbl_extend('force', defaults,
      { prompt_prefix = 'rg: ', }))
  end)
  keymap_set('n', '<leader>w', function()
    telescope.extensions.live_grep_args.live_grep_args(vim.tbl_extend('force', defaults,
      { prompt_prefix = 'rgw: ', default_text = vim.fn.expand('<cword>'), postfix = '', }))
  end)
  keymap_set('v', '<leader>w', function()
    telescope.extensions.live_grep_args.live_grep_args(vim.tbl_extend('force', defaults,
      { prompt_prefix = 'rgw: ', default_text = require('utils').get_visual_selection(), postfix = '', }))
  end)
  keymap_set('n', '<leader>T', ':TodoTelescope<CR>')
  keymap_set('n', '<leader>l', telescope_builtin.resume)
  keymap_set('n', 'ga', function()
    telescope_builtin.buffers({
      previewer = false,
      layout_config = { width = 0.0, height = 0.0, },
      on_complete = {
        function() vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes('<cr>', true, true, true), 'i', {}) end,
      },
    })
  end)
end

function M.oil()
  keymap_set('n', '<leader>F', ':Oil --float<cr>')
end

function M.attempt(attempt)
  keymap_set('n', '<leader>n', attempt.new_input_ext)
end

function M.close_buffers(close_buffers)
  keymap_set('n', '<leader>o', function() close_buffers.wipe({ type = 'other', }) end)
  keymap_set('n', '<leader>O', function() close_buffers.wipe({ type = 'other', force = true, }) end)
end

function M.delimited(delimited)
  keymap_set('n', 'dp', delimited.goto_prev)
  keymap_set('n', 'dn', delimited.goto_next)
end

function M.gitlinker()
  keymap_set({ 'n', 'v', }, '<leader>yl', ':GitLink<cr>')
  keymap_set({ 'n', 'v', }, '<leader>yL', ':GitLink!<cr>')
  keymap_set({ 'n', 'v', }, '<leader>yb', ':GitLink blame<cr>')
  keymap_set({ 'n', 'v', }, '<leader>yB', ':GitLink! blame<cr>')
end

function M.gitsigns(gitsigns)
  keymap_set('n', 'cn', function()
    if vim.wo.diff then return 'cn' end
    vim.schedule(function()
      gitsigns.next_hunk({ wrap = true, })
    end)
    return '<Ignore>'
  end, { expr = true, })

  keymap_set('n', 'cp', function()
    if vim.wo.diff then return 'cp' end
    vim.schedule(function()
      gitsigns.prev_hunk({ wrap = true, })
    end)
    return '<Ignore>'
  end, { expr = true, })

  keymap_set('n', '<leader>hs', gitsigns.stage_hunk)
  keymap_set('n', '<leader>hr', gitsigns.reset_hunk)
  keymap_set('v', '<leader>hs', function() gitsigns.stage_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
  keymap_set('v', '<leader>hr', function() gitsigns.reset_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end)
  keymap_set('n', '<leader>hu', gitsigns.undo_stage_hunk)
  keymap_set({ 'n', 'v', }, '<c-b>', function() gitsigns.blame_line({ full = true, }) end)
end

function M.lspconfig()
  return function(_, bufnr)
    keymap_set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr, })
    keymap_set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr, })
    keymap_set('n', '<leader>a', vim.lsp.buf.code_action, { buffer = bufnr, })
  end
end

return M
