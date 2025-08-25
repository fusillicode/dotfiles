local rua = require('rua')
local rua2 = require('rua2')

vim.diagnostic.config({
  float = {
    anchor_bias = 'above',
    border = require('colorscheme').window.border,
    focusable = true,
    format = rua2.format_diagnostic,
    header = '',
    prefix = '',
    source = false,
    suffix = '',
  },
  severity_sort = true,
  signs = true,
  underline = true,
  update_in_insert = false,
  virtual_text = false,
})

local diag_set = vim.diagnostic.set
vim.diagnostic.set = function(namespace, bufnr, diagnostics, opts)
  -- NOTE: enable this line to understand what's happening with diagnostics
  -- require('utils').log(diagnostics)
  -- NOTE: switch to this line if `rua.filter_diagnostics(diagnostics)` misbehave
  -- diag_set(namespace, bufnr, diagnostics, opts)
  diag_set(
    namespace,
    bufnr,
    rua2.sort_diagnostics(
      rua.filter_diagnostics(vim.api.nvim_buf_get_name(bufnr), diagnostics)
    ),
    opts
  )
end
