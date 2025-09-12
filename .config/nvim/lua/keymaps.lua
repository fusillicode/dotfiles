local M = {}

local nvrim = require('nvrim')

local function keymap_set(mode, lhs, rhs, opts)
  vim.keymap.set(mode, lhs, rhs, vim.tbl_extend('error', { silent = true, }, opts or {}))
end

function M.set_lua_implemented()
  local base_opts = { expr = true, }

  keymap_set('n', 'i', nvrim.keymaps.smart_ident_on_blank_line, base_opts)
  keymap_set('n', 'dd', nvrim.keymaps.smart_dd_no_yank_empty_line, base_opts)
  keymap_set('v', '<esc>', nvrim.keymaps.visual_esc, base_opts)
  keymap_set({ 'n', 'v', }, '<leader>t', nvrim.truster.run_test)
  keymap_set('n', 'gx', require('opener').open_under_cursor)

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

  local min_diag_level = vim.diagnostic.severity.WARN
  keymap_set('n', 'dn', function() vim.diagnostic.jump({ count = 1, severity = min_diag_level, }) end)
  keymap_set('n', 'dp', function() vim.diagnostic.jump({ count = -1, severity = min_diag_level, }) end)
  keymap_set('n', '<leader>e', vim.diagnostic.open_float)
end

function M.lspconfig(bufnr)
  keymap_set({ 'n', 'v', }, 'z', function()
    nvrim.buffer.foo()
  end, { buffer = bufnr, })
  keymap_set('n', 'K', vim.lsp.buf.hover, { buffer = bufnr, })
  keymap_set('n', '<leader>r', vim.lsp.buf.rename, { buffer = bufnr, })
end

function M.fzf_lua(fl)
  local lsp_cfg = { ignore_current_line = true, jump1 = true, includeDeclaration = false, }

  return {
    {
      'gd',
      mode = { 'n', 'v', },
      fl and { function() fl.lsp_definitions(vim.tbl_extend('error', { prompt = 'LSP defs: ', }, lsp_cfg)) end, },
    },
    {
      'gr',
      mode = { 'n', 'v', },
      fl and { function() fl.lsp_references(vim.tbl_extend('error', { prompt = 'LSP refs: ', }, lsp_cfg)) end, },
    },
    {
      'gi',
      mode = { 'n', 'v', },
      fl and { function() fl.lsp_implementations(vim.tbl_extend('error', { prompt = 'LSP impls: ', }, lsp_cfg)) end, },
    },
    { '<leader>a',  mode = { 'n', 'v', }, fl and { function() fl.lsp_code_actions({ prompt = 'LSP actions: ', }) end, }, },
    { '<leader>s',  mode = { 'n', 'v', }, fl and { function() fl.lsp_document_symbols({ prompt = 'LSP syms: ', }) end, }, },
    { '<leader>S',  mode = { 'n', 'v', }, fl and { function() fl.lsp_workspace_symbols({ prompt = '*LSP syms: ', }) end, }, },

    { '<leader>f',  mode = { 'n', 'v', }, fl and { function() fl.files({ prompt = 'Files: ', }) end, }, },
    { '<leader>b',  mode = { 'n', 'v', }, fl and { function() fl.buffers({ prompt = 'Buffers: ', }) end, }, },
    { '<leader>gs', mode = { 'n', 'v', }, fl and { function() fl.git_status({ prompt = 'Git status: ', }) end, }, },
    { '<leader>gc', mode = { 'n', 'v', }, fl and { function() fl.git_commits({ prompt = 'Git commits: ', }) end, }, },
    { '<leader>c',  mode = { 'n', 'v', }, fl and { function() fl.commands({ prompt = 'Cmds: ', }) end, }, },

    { '<leader>d',  mode = { 'n', 'v', }, fl and { function() fl.diagnostics_document({ prompt = 'Diags: ', }) end, }, },
    { '<leader>D',  mode = { 'n', 'v', }, fl and { function() fl.diagnostics_workspace({ prompt = '*Diags: ', sort = 0, }) end, }, },

    { '<leader>w',  mode = 'n',           fl and { function() fl.live_grep({ prompt = 'rg: ', }) end, }, },
    {
      '<leader>w',
      mode = 'v',
      fl and { function() fl.live_grep({ prompt = 'rg: ', search = nvrim.buffer.get_visual_selection()[1], }) end, },
    },

    { '<leader>h', mode = 'n', fl and { function() fl.resume({}) end, }, },
  }
end

function M.oil(oil)
  return {
    { '<leader>F', mode = 'n', oil and { ':Oil --float<cr>', }, },
  }
end

function M.attempt(att)
  return {
    { '<leader>n', mode = 'n', att and { att.new_select, }, },
  }
end

function M.close_buffers(cb)
  return {
    { '<leader>o', mode = 'n', cb and { function() cb.wipe({ type = 'other', }) end, }, },
    { '<leader>O', mode = 'n', cb and { function() cb.wipe({ type = 'other', force = true, }) end, }, },
  }
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
      mode = 'n',
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
      mode = 'n',
      gs and {
        function()
          if vim.wo.diff then return 'cp' end
          vim.schedule(function() gs.prev_hunk({ wrap = true, }) end)
          return '<Ignore>'
        end,
        { expr = true, },
      },
    },
    { '<leader>hd', mode = 'n',           gs and { gs.preview_hunk, }, },
    { '<leader>hs', mode = 'n',           gs and { gs.stage_hunk, }, },
    { '<leader>hr', mode = 'n',           gs and { gs.reset_hunk, }, },
    { '<leader>hs', mode = 'v',           gs and { function() gs.stage_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end, }, },
    { '<leader>hr', mode = 'v',           gs and { function() gs.reset_hunk({ vim.fn.line('.'), vim.fn.line('v'), }) end, }, },
    { '<leader>hu', mode = 'n',           gs and { gs.undo_stage_hunk, }, },
    { '<c-b>',      mode = { 'n', 'v', }, gs and { function() gs.blame_line({ full = true, }) end, }, },
  }
end

function M.multicursor(mc)
  return {
    {
      '<esc>',
      mode = 'n',
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
      mode = 'n',
      gf and {
        function() gf.open(vim.tbl_deep_extend('force', opts, {})) end,
      },
    },
    {
      '<leader>l',
      mode = 'v',
      gf and {
        function()
          local selection = require('utils').escape_regex(nvrim.buffer.get_visual_selection()[1])
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

function M.opencode(oc)
  return {
    { '<leader>oA', mode = 'n',           oc and { function() require('opencode').ask() end, desc = 'Ask opencode', }, },
    { '<leader>oa', mode = 'v',           oc and { function() require('opencode').ask('@selection: ') end, desc = 'Ask opencode about selection', }, },
    { '<leader>on', mode = 'n',           oc and { function() require('opencode').command('session_new') end, desc = 'New session', }, },
    { '<leader>oy', mode = 'n',           oc and { function() require('opencode').command('messages_copy') end, desc = 'Copy last message', }, },
    { '<leader>op', mode = { 'n', 'v', }, oc and { function() require('opencode').select_prompt() end, desc = 'Select prompt', }, },
    {
      '<leader>oe',
      mode = 'n',
      oc and {
        function() require('opencode').prompt('Explain @cursor and its context') end,
        desc = 'Explain code near cursor',
      },
    },
  }
end

function M.text_transform()
  return {
    { '<leader>u', mode = { 'n', 'v', }, },
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
