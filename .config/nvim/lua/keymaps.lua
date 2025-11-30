local M = {}

local nvrim = require('nvrim')

local function keymap_set(mode, lhs, rhs, opts)
  vim.keymap.set(mode, lhs, rhs, vim.tbl_extend('error', { silent = true, }, opts or {}))
end

function M.set_lua_defined()
  local base_opts = { expr = true, }

  keymap_set('n', 'i', nvrim.keymaps.smart_ident_on_blank_line, base_opts)
  keymap_set('n', 'dd', nvrim.keymaps.smart_dd_no_yank_empty_line, base_opts)
  -- TODO: is this really needed?
  keymap_set('v', '<esc>', nvrim.keymaps.visual_esc, base_opts)
  keymap_set({ 'n', 'v', }, '<leader>t', nvrim.plugins.truster.run_test)
  keymap_set('n', 'gx', require('opener').open_under_cursor)

  keymap_set({ 'n', 'v', 'i', 't', }, '<c-e>', nvrim.layout.focus_term)
  keymap_set({ 'n', 'v', 'i', 't', }, '<c-h>', nvrim.layout.focus_buffer)
  keymap_set({ 'n', 'v', }, 'ga', nvrim.layout.toggle_alternate_buffer)
  keymap_set({ 'n', 'v', }, '<leader>x', function() nvrim.layout.smart_close_buffer() end)
  keymap_set({ 'n', 'v', }, '<leader>X', function() nvrim.layout.smart_close_buffer(true) end)

  local min_diag_level = vim.diagnostic.severity.ERROR
  keymap_set('n', 'dn', function() vim.diagnostic.jump({ count = 1, severity = min_diag_level, }) end)
  keymap_set('n', 'dp', function() vim.diagnostic.jump({ count = -1, severity = min_diag_level, }) end)
  keymap_set('n', '<leader>e', vim.diagnostic.open_float)
end

function M.lspconfig(bufnr)
  keymap_set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr, })
  keymap_set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr, })
end

function M.fzf_lua(plugin)
  local lsp_cfg = { ignore_current_line = true, jump1 = true, includeDeclaration = false, }

  return {
    {
      'gd',
      mode = { 'n', 'v', },
      plugin and { function() plugin.lsp_definitions(vim.tbl_extend('error', { prompt = 'LSP defs: ', }, lsp_cfg)) end, },
    },
    {
      'gr',
      mode = { 'n', 'v', },
      plugin and { function() plugin.lsp_references(vim.tbl_extend('error', { prompt = 'LSP refs: ', }, lsp_cfg)) end, },
    },
    {
      'gi',
      mode = { 'n', 'v', },
      plugin and
      { function() plugin.lsp_implementations(vim.tbl_extend('error', { prompt = 'LSP impls: ', }, lsp_cfg)) end, },
    },
    { '<leader>a',  mode = { 'n', 'v', }, plugin and { function() plugin.lsp_code_actions({ prompt = 'LSP actions: ', }) end, }, },
    { '<leader>s',  mode = { 'n', 'v', }, plugin and { function() plugin.lsp_document_symbols({ prompt = 'LSP syms: ', }) end, }, },
    { '<leader>S',  mode = { 'n', 'v', }, plugin and { function() plugin.lsp_workspace_symbols({ prompt = '*LSP syms: ', }) end, }, },

    { '<leader>f',  mode = { 'n', 'v', }, plugin and { function() plugin.files({ prompt = 'Files: ', }) end, }, },
    { '<leader>b',  mode = { 'n', 'v', }, plugin and { function() plugin.buffers({ prompt = 'Buffers: ', }) end, }, },
    { '<leader>gs', mode = { 'n', 'v', }, plugin and { function() plugin.git_status({ prompt = 'Git status: ', }) end, }, },
    { '<leader>gc', mode = { 'n', 'v', }, plugin and { function() plugin.git_commits({ prompt = 'Git commits: ', }) end, }, },
    { '<leader>c',  mode = { 'n', 'v', }, plugin and { function() plugin.commands({ prompt = 'Cmds: ', }) end, }, },

    { '<leader>d',  mode = { 'n', 'v', }, plugin and { function() plugin.diagnostics_document({ prompt = 'Diags: ', }) end, }, },
    { '<leader>D',  mode = { 'n', 'v', }, plugin and { function() plugin.diagnostics_workspace({ prompt = '*Diags: ', sort = 0, }) end, }, },

    { '<leader>w',  mode = 'n',           plugin and { function() plugin.live_grep({ prompt = '*rg: ', }) end, }, },
    { '<leader>/',  mode = 'n',           plugin and { function() plugin.lgrep_curbuf({ prompt = 'rg: ', }) end, }, },
    {
      '<leader>w',
      mode = 'v',
      plugin and
      { function() plugin.live_grep({ prompt = 'rg: ', search = nvrim.buffer.get_visual_selection_lines()[1], }) end, },
    },

    { '<leader>h',  mode = 'n',           plugin and { function() plugin.resume({}) end, }, },
    { '<leader>n',  mode = 'n',           plugin and { nvrim.plugins.attempt.create_scratch_file, }, },
    { '<leader>u',  mode = 'v',           plugin and { nvrim.plugins.caseconv.convert_selection, }, },
    { '<leader>k',  mode = 'v',           plugin and { nvrim.plugins.genconv.convert_selection, }, },
    { '<leader>yl', mode = { 'n', 'v', }, plugin and { function() nvrim.plugins.ghurlinker.get_link('blob') end, }, },
    { '<leader>yb', mode = { 'n', 'v', }, plugin and { function() nvrim.plugins.ghurlinker.get_link('blame') end, }, },
    { '<leader>yL', mode = { 'n', 'v', }, plugin and { function() nvrim.plugins.ghurlinker.get_link('blob', true) end, }, },
    { '<leader>yB', mode = { 'n', 'v', }, plugin and { function() nvrim.plugins.ghurlinker.get_link('blame', true) end, }, },
    { '<leader>gh', mode = { 'n', 'v', }, plugin and { nvrim.plugins.gdiff.get_hunks, }, },
    { '<leader>gH', mode = { 'n', 'v', }, plugin and { function() nvrim.plugins.gdiff.get_hunks(true) end, }, },
  }
end

function M.oil(plugin)
  return {
    { '<leader>F', mode = 'n', plugin and { ':Oil --float<cr>', }, },
  }
end

function M.close_buffers(plugin)
  return {
    { '<leader>o', mode = 'n', plugin and { function() plugin.wipe({ type = 'other', }) end, }, },
    { '<leader>O', mode = 'n', plugin and { function() plugin.wipe({ type = 'other', force = true, }) end, }, },
  }
end

function M.gitsigns(plugin)
  return {
    {
      'cn',
      mode = 'n',
      plugin and {
        function()
          if vim.wo.diff then return 'cn' end
          vim.schedule(function() plugin.next_hunk({ wrap = true, }) end)
          return '<Ignore>'
        end,
        { expr = true, },
      },
    },
    {
      'cp',
      mode = 'n',
      plugin and {
        function()
          if vim.wo.diff then return 'cp' end
          vim.schedule(function() plugin.prev_hunk({ wrap = true, }) end)
          return '<Ignore>'
        end,
        { expr = true, },
      },
    },
    { '<leader>hd', mode = 'n', plugin and { plugin.preview_hunk, }, },
    { '<leader>hs', mode = 'n', plugin and { plugin.stage_hunk, }, },
    { '<leader>hr', mode = 'n', plugin and { plugin.reset_hunk, }, },
    { '<leader>hs', mode = 'v', plugin and { function() plugin.stage_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end, }, },
    { '<leader>hr', mode = 'v', plugin and { function() plugin.reset_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end, }, },
    { '<leader>hu', mode = 'n', plugin and { plugin.undo_stage_hunk, }, },
  }
end

function M.multicursor(plugin)
  return {
    {
      '<esc>',
      mode = 'n',
      plugin and {
        function()
          if not plugin.cursorsEnabled() then return plugin.enableCursors() end
          if plugin.hasCursors() then return plugin.clearCursors() end
          vim.api.nvim_command('noh | echo""')
        end,
      },
    },
    { '<c-j>', mode = { 'n', 'v', }, plugin and { function() plugin.addCursor('j') end, }, },
    { '<c-k>', mode = { 'n', 'v', }, plugin and { function() plugin.addCursor('k') end, }, },
    { '<c-n>', mode = { 'n', 'v', }, plugin and { function() plugin.matchAddCursor(1) end, }, },
    { '<c-p>', mode = { 'n', 'v', }, plugin and { function() plugin.matchAddCursor(-1) end, }, },
  }
end

function M.grug_far(plugin, opts)
  return {
    {
      '<leader>l',
      mode = 'n',
      plugin and {
        function() plugin.open(vim.tbl_deep_extend('force', opts, {})) end,
      },
    },
    {
      '<leader>l',
      mode = 'v',
      plugin and {
        function()
          local selection = require('utils').escape_regex(nvrim.buffer.get_visual_selection_lines()[1])
          plugin.open(vim.tbl_deep_extend('force', opts, { prefills = { search = selection, }, }))
        end,
      },
    },
  }
end

function M.nvim_spider(plugin)
  return {
    { 'w', mode = { 'n', 'o', 'x', }, plugin and { function() plugin.motion('w') end, }, },
    { 'e', mode = { 'n', 'o', 'x', }, plugin and { function() plugin.motion('e') end, }, },
    { 'b', mode = { 'n', 'o', 'x', }, plugin and { function() plugin.motion('b') end, }, },
  }
end

function M.opencode(plugin)
  return {
    { '<leader>oA', mode = 'n',           plugin and { function() require('opencode').ask() end, desc = 'Ask opencode', }, },
    {
      '<leader>oa',
      mode = 'v',
      plugin and {
        function()
          require('opencode').ask('@this: ',
            { submit = true, })
        end,
        desc = 'Ask opencode about selection',
      },
    },
    { '<leader>on', mode = 'n',           plugin and { function() require('opencode').command('session_new') end, desc = 'New session', }, },
    { '<leader>oy', mode = 'n',           plugin and { function() require('opencode').command('messages_copy') end, desc = 'Copy last message', }, },
    { '<leader>op', mode = { 'n', 'v', }, plugin and { function() require('opencode').select_prompt() end, desc = 'Select prompt', }, },
    {
      '<leader>oe',
      mode = 'n',
      plugin and {
        function() require('opencode').prompt('Explain @cursor and its context') end,
        desc = 'Explain code near cursor',
      },
    },
  }
end

function M.set(keymaps)
  for idx, keymap in ipairs(keymaps) do
    local lhs = keymap[1]
    local mode = keymap.mode
    local payload = keymap[2]

    if type(lhs) ~= 'string' then
      error(('apply_keymaps[%d]: lhs must be string'):format(idx))
    end
    local mode_t = type(mode)
    if not (mode_t == 'table' or mode_t == 'string') then
      error(('apply_keymaps[%d]: mode must be a list or a string for %q'):format(idx, lhs))
    end
    if type(payload) ~= 'table' then
      error(('apply_keymaps[%d]: payload must be a table for %q'):format(idx, lhs))
    end

    local rhs = payload[1]
    local opts = payload[2]

    if not (type(rhs) == 'function' or type(rhs) == 'string') then
      error(('apply_keymaps[%d]: payload[1] must be function|string for %q'):format(idx, lhs))
    end
    if opts ~= nil and type(opts) ~= 'table' then
      error(('apply_keymaps[%d]: payload[2] must be table when present for %q'):format(idx, lhs))
    end

    keymap_set(mode, lhs, rhs, opts or {})
  end
end

return M
