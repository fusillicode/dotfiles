local M = {}

local function keymap_set(modes, lhs, rhs, opts)
  vim.keymap.set(modes, lhs, rhs, vim.tbl_extend('force', { silent = true, }, opts or {}))
end

local function get_enclosing_fn_or_method_name()
  local node = require('nvim-treesitter.ts_utils').get_node_at_cursor()
  if not node then return end

  while node do
    local node_type = node:type()
    if node_type == 'function_definition' or node_type == 'method_definition' or
        node_type == 'function_declaration' or node_type == 'method_declaration' or
        node_type == 'function' or node_type == 'method' or node_type == 'function_item' then
      local name_node = node:field('name')[1]
      if name_node then return vim.treesitter.get_node_text(name_node, 0) end
    end
    node = node:parent()
  end
end

local function yank_enclosing_fn_or_method_name()
  local name = get_enclosing_fn_or_method_name()
  if name then vim.fn.setreg('+', name) end
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
  keymap_set({ 'n', 'v', }, 'X', '"_X')

  keymap_set({ 'n', 'v', }, '<leader>yf', ':let @+ = expand("%") . ":" . line(".")<cr>')
  keymap_set({ 'n', 'v', }, '<leader>yc', yank_enclosing_fn_or_method_name)
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

  keymap_set('n', 'dn', function() vim.diagnostic.goto_next() end)
  keymap_set('n', 'dp', function() vim.diagnostic.goto_prev() end)
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

function M.fzf_lua(fzf_lua)
  keymap_set('n', '<leader>c', function() fzf_lua.commands() end)
  keymap_set('n', '<leader>f', function() fzf_lua.files({ prompt = 'Files: ', }) end)
  keymap_set('n', '<leader>b', function() fzf_lua.buffers({ prompt = 'Buffers: ', }) end)
  keymap_set('n', '<leader>gs', function() fzf_lua.git_status({ prompt = 'Changes: ', }) end)
  keymap_set('n', '<leader>gc', function() fzf_lua.git_commits({ prompt = 'Commits: ', }) end)
  keymap_set('n', '<leader>gcc', function() fzf_lua.git_bcommits({ prompt = 'Buffer commits: ', }) end)
  keymap_set('n', '<leader>gb', function() fzf_lua.git_branches({ prompt = 'Branches: ', }) end)

  local lsp_jumps_cfg = { ignore_current_line = true, jump_to_single_result = true, }
  keymap_set('n', 'gr', function()
    fzf_lua.lsp_references(vim.tbl_extend('error', { prompt = 'References: ', }, lsp_jumps_cfg))
  end)
  keymap_set('n', 'gd', function()
    fzf_lua.lsp_definitions(vim.tbl_extend('error', { prompt = 'Definitions: ', }, lsp_jumps_cfg))
  end)
  keymap_set('n', 'gi', function()
    fzf_lua.lsp_implementations(vim.tbl_extend('error', { prompt = 'Implementations: ', }, lsp_jumps_cfg))
  end)
  keymap_set('n', 'go', function()
    fzf_lua.lsp_outgoing_calls(vim.tbl_extend('error', { prompt = 'Out calls: ', }, lsp_jumps_cfg))
  end)
  keymap_set('n', 'gz', function()
    fzf_lua.lsp_incoming_calls(vim.tbl_extend('error', { prompt = 'In calls: ', }, lsp_jumps_cfg))
  end)

  keymap_set('n', '<leader>j', function() fzf_lua.jumps() end)
  keymap_set('n', '<leader>s', function() fzf_lua.lsp_document_symbols({ prompt = 'Buffer symbols: ', }) end)
  keymap_set('n', '<leader>S', function() fzf_lua.lsp_live_workspace_symbols({ prompt = 'Workspace symbols: ', }) end)
  keymap_set('n', '<leader>a', function() fzf_lua.lsp_code_actions({ prompt = 'Code actions: ', }) end)
  keymap_set('n', '<leader>d', function() fzf_lua.diagnostics_document({ prompt = 'Buffer diagnostics: ', }) end)
  keymap_set('n', '<leader>D', function() fzf_lua.diagnostics_workspace({ prompt = 'Workspace diagnostics: ', }) end)

  keymap_set('n', '<leader>w', function() fzf_lua.live_grep_glob({ prompt = 'rg: ', }) end)
  keymap_set('v', '<leader>w', function()
    local selection = fzf_lua.utils.get_visual_selection()
    if selection then fzf_lua.live_grep_glob({ prompt = 'rg: ', query = selection, }) end
  end)

  local todo_comments_cfg = {
    search =
    ' FIX:| FIXME:| BUG:| FIXIT:| ISSUE:| TODO:| HACK:| WARN:| WARNING:| PERF:| OPTIM:| PERFORMANCE:| OPTIMIZE:| NOTE:| INFO:',
    no_esc = true,
  }
  keymap_set('n', '<leader>t', function()
    fzf_lua.grep_curbuf(vim.tbl_extend('error', todo_comments_cfg, { prompt = 'Buffer TODOs: ', }))
  end)
  keymap_set('n', '<leader>T', function()
    fzf_lua.grep(vim.tbl_extend('error', todo_comments_cfg, { prompt = 'Workspace TODOs: ', }))
  end)

  keymap_set('n', '<leader>l', function() fzf_lua.resume() end)
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

function M.lspconfig(bufnr)
  keymap_set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr, })
  keymap_set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr, })
end

function M.grug_far(grug_far, opts)
  keymap_set('n', '<leader>/', function()
    grug_far(vim.tbl_deep_extend('force', opts, {}))
  end)
  keymap_set('v', '<leader>/', function()
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
