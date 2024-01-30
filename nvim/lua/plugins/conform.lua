return {
  'stevearc/conform.nvim',
  event = 'BufWritePre',
  cmd = 'ConformInfo',
  opts = {
    formatters_by_ft = {
      javascript = { { 'prettier', }, },
      typescript = { { 'prettier', }, },
      typescriptreact = { { 'prettier', }, },
      ['*'] = { 'trim_whitespaces', 'trim_newlines', },
    },
    format_on_save = { timeout_ms = 500, lsp_fallback = true, },
  },
}
