local M = {}

local function keymap_set(modes, lhs, rhs, opts)
  vim.keymap.set(modes, lhs, rhs, vim.tbl_extend('force', { silent = true, }, opts or {}))
end

function M.core()
  vim.g.mapleader = ' '
  vim.g.maplocalleader = ' '

  keymap_set('t', '<Esc>', '<C-\\><C-n>')

  -- https://stackoverflow.com/a/3003636
  keymap_set('n', 'i', function()
    return (vim.fn.empty(vim.fn.getline('.')) == 1 and '\"_cc' or 'i')
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
    return (vim.api.nvim_get_current_line():match('^%s*$') and '"_dd' or 'dd')
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
  keymap_set({ 'n', 'v', }, '<leader>W', ':wa!<cr>')
  keymap_set({ 'n', 'v', }, '<leader>q', ':q<cr>')
  keymap_set({ 'n', 'v', }, '<leader>Q', ':q!<cr>')

  keymap_set({ 'n', 'v', }, '<c-;>', ':set wrap!<cr>')
  keymap_set('n', '<esc>', require('utils').normal_esc)
  keymap_set('v', '<esc>', require('utils').visual_esc, { expr = true, })

  keymap_set('n', 'dn', function()
    local errs = vim.tbl_filter(function(diag)
      return diag.severity == vim.diagnostic.severity.ERROR
    end, vim.diagnostic.get(0))

    if #errs > 0 then vim.diagnostic.goto_next({ severity = vim.diagnostic.severity.ERROR, }) end
  end)
  keymap_set('n', 'dp', function()
    local errs = vim.tbl_filter(function(diag)
      return diag.severity == vim.diagnostic.severity.ERROR
    end, vim.diagnostic.get(0))

    if #errs > 0 then vim.diagnostic.goto_prev({ severity = vim.diagnostic.severity.ERROR, }) end
  end)
  keymap_set('n', '<leader>e', vim.diagnostic.open_float)

  keymap_set('n', '<leader>gx', require('opener').open_under_cursor)
  keymap_set('v', '<leader>gx', require('opener').open_selection)

  keymap_set('v', '<leader>/', function()
    local start_ln, _, end_ln, _ = require('utils').get_visual_selection_boundaries()

    local search = vim.fn.escape(vim.fn.input('Search: '), '/')
    if search == '' then
      print('No search')
      return
    end

    local replace = vim.fn.escape(vim.fn.input('Replace: '), '/')
    if replace == '' then
      print('No replace')
      return
    end

    vim.cmd(start_ln .. ',' .. end_ln .. 's/' .. search .. '/' .. replace .. '/g')
  end)
end

function M.telescope(telescope_builtin, defaults)
  local function with_defaults(picker, opts)
    return function()
      telescope_builtin[picker](vim.tbl_extend('force', defaults, opts or {}))
    end
  end

  keymap_set({ 'n', 'v', }, 'gd', with_defaults('lsp_definitions', { prompt_prefix = 'LSP Defs: ', }))
  keymap_set({ 'n', 'v', }, 'gr', with_defaults('lsp_references', { prompt_prefix = 'LSP Refs: ', }))
  keymap_set({ 'n', 'v', }, 'gi', with_defaults('lsp_implementations', { prompt_prefix = 'LSP Impls: ', }))
  keymap_set({ 'n', 'v', }, '<leader>s', with_defaults('lsp_document_symbols', { prompt_prefix = 'LSP Syms: ', }))
  keymap_set({ 'n', 'v', }, '<leader>S',
    with_defaults('lsp_dynamic_workspace_symbols', { prompt_prefix = 'LSP Syms*: ', }))
  keymap_set({ 'n', 'v', }, '<leader>f', with_defaults('find_files', { prompt_prefix = 'Files: ', }))
  keymap_set({ 'n', 'v', }, '<leader>b', with_defaults('buffers', { prompt_prefix = 'Bufs: ', }))

  keymap_set('n', '<leader>w', function()
    require('telescope').extensions.live_grep_args.live_grep_args(
      { prompt_title = false, prompt_prefix = 'rg: ', }
    )
  end)
  keymap_set('v', '<leader>w', function()
    require('telescope-live-grep-args.shortcuts').grep_visual_selection(
      { prompt_title = false, prompt_prefix = 'rg: ', }
    )
  end)

  keymap_set({ 'n', 'v', }, '<leader>gc', with_defaults('git_commits', { prompt_prefix = 'gc*: ', }))
  keymap_set({ 'n', 'v', }, '<leader>gcb', with_defaults('git_bcommits', { prompt_prefix = 'gc: ', bufnr = 0, }))
  keymap_set({ 'n', 'v', }, '<leader>gb', with_defaults('git_branches', { prompt_prefix = 'gb: ', }))
  keymap_set({ 'n', 'v', }, '<leader>gs', with_defaults('git_status', { prompt_prefix = 'gst: ', }))
  keymap_set({ 'n', 'v', }, '<leader>d',
    with_defaults('diagnostics', { prompt_prefix = 'Diagn: ', bufnr = 0, sort_by = 'severity', }))
  keymap_set({ 'n', 'v', }, '<leader>D',
    with_defaults('diagnostics', { prompt_prefix = 'Diagn*: ', sort_by = 'severity', }))
  keymap_set({ 'n', 'v', }, '<leader>c', with_defaults('commands', { prompt_prefix = 'Cmds: ', }))
  keymap_set({ 'n', 'v', }, '<leader>T', function()
    require('telescope').extensions.live_grep_args.live_grep_args(
      {
        prompt_title = false,
        default_text =
        'FIX:|FIXME:|BUG:|FIXIT:|ISSUE:|TODO:|HACK:|WARN:|WARNING:|PERF:|OPTIM:|PERFORMANCE:|OPTIMIZE:|NOTE|:INFO',
      })
  end)
  keymap_set({ 'n', 'v', }, 'ga', function()
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
  keymap_set('n', '<leader>n', attempt.new_select)
end

function M.close_buffers(close_buffers)
  keymap_set('n', '<leader>o', function() close_buffers.wipe({ type = 'other', }) end)
  keymap_set('n', '<leader>O', function() close_buffers.wipe({ type = 'other', force = true, }) end)
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

  keymap_set('n', '<leader>hd', gitsigns.preview_hunk)
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

function M.grug_far(grug_far, opts)
  keymap_set('n', '<leader>l', function()
    grug_far(vim.tbl_deep_extend('force', opts, {}))
  end)
  keymap_set('v', '<leader>l', function()
    local utils = require('utils')
    local selection = utils.escape_regex(utils.get_visual_selection())
    grug_far(vim.tbl_deep_extend('force', opts, { prefills = { search = selection, }, }))
  end)
end

function M.multicursor(mc)
  keymap_set('n', '<esc>', function()
    if not mc.cursorsEnabled() then return mc.enableCursors() end
    if mc.hasCursors() then return mc.clearCursors() end
    vim.api.nvim_command('noh | echo""')
  end)

  keymap_set({ 'n', 'v', }, '<c-j>', function() mc.addCursor('j') end)
  keymap_set({ 'n', 'v', }, '<c-k>', function() mc.addCursor('k') end)
  keymap_set({ 'n', 'v', }, '<c-n>', function() mc.matchAddCursor(1) end)
  keymap_set({ 'n', 'v', }, '<c-p>', function() mc.matchAddCursor(-1) end)
end

function M.quickfix()
  local opts = { noremap = true, buffer = true, }
  keymap_set('n', '<c-n>', ':cn<cr>', opts)
  keymap_set('n', '<c-p>', ':cp<cr>', opts)
  keymap_set('n', '<c-x>', ':ccl<cr>', opts)
end

return M
