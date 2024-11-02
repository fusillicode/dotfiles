vim.diagnostic.config {
  float = {
    anchor_bias = 'above',
    border = 'rounded',
    focusable = true,
    format = require('rua').format_diagnostic,
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
