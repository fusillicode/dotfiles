local M = {}

local rua = require('rua')

local function keymap_set(modes, lhs, rhs, opts)
  vim.keymap.set(modes, lhs, rhs, vim.tbl_extend('force', { silent = true, }, opts or {}))
end

function M.core()
  vim.g.mapleader = ' '
  vim.g.maplocalleader = ' '

  keymap_set('t', '<Esc>', '<c-\\><c-n>')

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
  keymap_set({ 'n', 'v', }, '<leader>q', ':q<cr>')
  keymap_set({ 'n', 'v', }, '<leader>Q', ':q!<cr>')

  keymap_set({ 'n', 'v', }, '<c-;>', ':set wrap!<cr>')
  keymap_set('n', '<esc>', require('utils').normal_esc)
  keymap_set('v', '<esc>', require('utils').visual_esc, { expr = true, })

  keymap_set('n', 'dn', function() vim.diagnostic.goto_next() end)
  keymap_set('n', 'dp', function() vim.diagnostic.goto_prev() end)
  keymap_set('n', '<leader>e', vim.diagnostic.open_float)

  keymap_set('n', '<leader>gx', require('opener').open_under_cursor)

  -- Thanks perplexity 🥲
  keymap_set({ 'n', 'v', }, 'ga', function()
    local alt_buf = vim.fn.bufnr('#')
    -- If alternate buffer valid, loaded, and listed, switch to it
    if alt_buf ~= -1 and vim.api.nvim_buf_is_loaded(alt_buf) and vim.fn.buflisted(alt_buf) == 1 then
      vim.api.nvim_set_current_buf(alt_buf)
      return
    end

    -- Otherwise, get list of loaded & listed buffers
    local bufs = vim.fn.getbufinfo({ bufloaded = true, listed = true, })
    local current_buf = vim.api.nvim_get_current_buf()
    if #bufs == 0 then return end
    -- Find the last buffer in the list that's not the current one
    for i = #bufs, 1, -1 do
      local bufnr = bufs[i].bufnr
      local buftype = vim.api.nvim_buf_get_option(bufnr, 'buftype')
      local bufname = vim.api.nvim_buf_get_name(bufnr)
      if bufnr ~= current_buf and buftype == '' and bufname ~= '' then
        vim.api.nvim_set_current_buf(bufnr)
        return
      end
    end
  end)

  keymap_set({ 'n', 'v', }, '<leader>t', rua.run_test)
end

function M.lspconfig(bufnr)
  keymap_set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr, })
  keymap_set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr, })
end

function M.quickfix()
  local opts = { noremap = true, buffer = true, }
  keymap_set('n', '<c-n>', ':cn<cr>', opts)
  keymap_set('n', '<c-p>', ':cp<cr>', opts)
  keymap_set('n', '<c-x>', ':ccl<cr>', opts)
end

function M.fzf_lua(fzf_lua)
  local lsp_cfg = { ignore_current_line = true, jump1 = true, includeDeclaration = false, }

  keymap_set({ 'n', 'v', }, 'gd', function()
    fzf_lua.lsp_definitions(vim.tbl_extend('error', { prompt = 'LSP defs: ', }, lsp_cfg))
  end)
  keymap_set({ 'n', 'v', }, 'gr', function()
    fzf_lua.lsp_references(vim.tbl_extend('error', { prompt = 'LSP refs: ', }, lsp_cfg))
  end)
  keymap_set({ 'n', 'v', }, 'gi', function()
    fzf_lua.lsp_implementations(vim.tbl_extend('error', { prompt = 'LSP impls: ', }, lsp_cfg))
  end)
  keymap_set({ 'n', 'v', }, '<leader>a', function() fzf_lua.lsp_code_actions({ prompt = 'LSP actions: ', }) end)
  keymap_set({ 'n', 'v', }, '<leader>s', function() fzf_lua.lsp_document_symbols({ prompt = 'LSP syms: ', }) end)
  keymap_set({ 'n', 'v', }, '<leader>S', function() fzf_lua.lsp_workspace_symbols({ prompt = '*LSP syms: ', }) end)

  keymap_set({ 'n', 'v', }, '<leader>f', function() fzf_lua.files({ prompt = 'Files: ', }) end)
  keymap_set({ 'n', 'v', }, '<leader>b', function() fzf_lua.buffers({ prompt = 'Buffers: ', }) end)
  keymap_set({ 'n', 'v', }, '<leader>gs', function() fzf_lua.git_status({ prompt = 'Git status: ', }) end)
  keymap_set({ 'n', 'v', }, '<leader>gc', function() fzf_lua.git_commits({ prompt = 'Git commits: ', }) end)
  keymap_set({ 'n', 'v', }, '<leader>c', function() fzf_lua.commands({ prompt = 'Cmds: ', }) end)
  keymap_set({ 'n', 'v', }, '<leader>d', function() fzf_lua.diagnostics_document({ prompt = 'Diags: ', }) end)
  -- Sorting of all workspace diagnostic is done with fzf-lua rather than with `rua` sorter because the sorter works
  -- at the buffer level
  keymap_set({ 'n', 'v', }, '<leader>D', function() fzf_lua.diagnostics_workspace({ prompt = '*Diags: ', sort = 0, }) end)

  keymap_set('n', '<leader>w', function() fzf_lua.live_grep({ prompt = 'rg: ', }) end)
  keymap_set('v', '<leader>w', function()
    fzf_lua.live_grep({ prompt = 'rg: ', search = rua.get_current_buffer_text(vim.fn.getpos('v'), vim.fn.getpos('.'))[1], })
  end)

  keymap_set('n', '<leader>h', function() fzf_lua.resume({}) end)
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

function M.gitlinker(gs)
  return {
    { '<leader>yl', mode = { 'n', 'v', }, gs and { ':GitLink<cr>', }, },
    { '<leader>yL', mode = { 'n', 'v', }, gs and { ':GitLink!<cr>', }, },
    { '<leader>yb', mode = { 'n', 'v', }, gs and { ':GitLink blame<cr>', }, },
    { '<leader>yB', mode = { 'n', 'v', }, gs and { ':GitLink! blame<cr>', }, },
  }
end

function M.gitsigns(gs)
  return {
    {
      'cn',
      mode = { 'n', },
      gs and {
        function()
          if vim.wo.diff then return 'cn' end
          vim.schedule(function() gs.next_hunk({ wrap = true, }) end)
          return '<Ignore>'
        end,
        { expr = true, },
      },
    },
    {
      'cp',
      mode = { 'n', },
      gs and {
        function()
          if vim.wo.diff then return 'cp' end
          vim.schedule(function() gs.prev_hunk({ wrap = true, }) end)
          return '<Ignore>'
        end,
        { expr = true, },
      },
    },
    { '<leader>hd', mode = { 'n', }, gs and { gs.preview_hunk, }, },
    { '<leader>hs', mode = { 'n', }, gs and { gs.stage_hunk, }, },
    { '<leader>hr', mode = { 'n', }, gs and { gs.reset_hunk, }, },
    {
      '<leader>hs',
      mode = { 'v', },
      gs and { function() gs.stage_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end, },
    },
    {
      '<leader>hr',
      mode = { 'v', },
      gs and { function() gs.reset_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end, },
    },
    { '<leader>hu', mode = { 'n', }, gs and { gs.undo_stage_hunk, }, },
    {
      '<c-b>',
      mode = { 'n', 'v', },
      gs and { function() gs.blame_line({ full = true, }) end, },
    },
  }
end

function M.multicursor(mc)
  return {
    {
      '<esc>',
      mode = { 'n', },
      mc and {
        function()
          if not mc.cursorsEnabled() then return mc.enableCursors() end
          if mc.hasCursors() then return mc.clearCursors() end
          vim.api.nvim_command('noh | echo""')
        end,
      },
    },
    { '<c-j>', mode = { 'n', 'v', }, mc and { function() mc.addCursor('j') end, }, },
    { '<c-k>', mode = { 'n', 'v', }, mc and { function() mc.addCursor('k') end, }, },
    { '<c-n>', mode = { 'n', 'v', }, mc and { function() mc.matchAddCursor(1) end, }, },
    { '<c-p>', mode = { 'n', 'v', }, mc and { function() mc.matchAddCursor(-1) end, }, },
  }
end

function M.grug_far(gf, opts)
  return {
    {
      '<leader>l',
      mode = { 'n', },
      gf and {
        function() gf.open(vim.tbl_deep_extend('force', opts, {})) end,
      },
    },
    {
      '<leader>l',
      mode = { 'v', },
      gf and {
        function()
          local selection = require('utils').escape_regex(
            rua.get_current_buffer_text(vim.fn.getpos('v'), vim.fn.getpos('.'))[1]
          )
          gf.open(vim.tbl_deep_extend('force', opts, { prefills = { search = selection, }, }))
        end,
      },
    },
  }
end

function M.nvim_spider(sp)
  return {
    { 'w', mode = { 'n', 'o', 'x', }, sp and { function() sp.motion('w') end, }, },
    { 'e', mode = { 'n', 'o', 'x', }, sp and { function() sp.motion('e') end, }, },
    { 'b', mode = { 'n', 'o', 'x', }, sp and { function() sp.motion('b') end, }, },
  }
end

function M.set(keymaps)
  for i, km in ipairs(keymaps) do
    local lhs = km[1]
    local modes = km.mode
    local payload = km[2]

    if type(lhs) ~= 'string' then
      error(('apply_keymaps[%d]: lhs must be string'):format(i))
    end
    if type(modes) ~= 'table' or #modes == 0 then
      error(('apply_keymaps[%d]: mode must be non-empty list for %q'):format(i, lhs))
    end
    if type(payload) ~= 'table' then
      error(('apply_keymaps[%d]: payload must be a table for %q'):format(i, lhs))
    end

    local rhs = payload[1]
    local opts = payload[2]

    if not (type(rhs) == 'function' or type(rhs) == 'string') then
      error(('apply_keymaps[%d]: payload[1] must be function|string for %q'):format(i, lhs))
    end
    if opts ~= nil and type(opts) ~= 'table' then
      error(('apply_keymaps[%d]: payload[2] must be table when present for %q'):format(i, lhs))
    end

    keymap_set(modes, lhs, rhs, opts or {})
  end
end

return M
