local rua = require('rua')

vim.diagnostic.config {
  float = {
    anchor_bias = 'above',
    border = 'rounded',
    focusable = true,
    format = rua.format_diagnostic,
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
}

local log = require('utils').log
local diag_set = vim.diagnostic.set
vim.diagnostic.set = function(namespace, bufnr, diagnostics, opts)
  log(diagnostics)
  diag_set(namespace, bufnr, rua.filter_diagnostics(diagnostics), opts)
  -- diag_set(namespace, bufnr, diagnostics, opts)
end
