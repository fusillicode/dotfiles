return {
  'stevearc/conform.nvim',
  event = 'BufWritePre',
  cmd = 'ConformInfo',
  opts = {
    formatters_by_ft = {
      css = { { 'prettierd', }, },
      graphql = { { 'prettierd', }, },
      html = { { 'prettierd', }, },
      javascript = { { 'prettierd', }, },
      markdown = { { 'prettierd', }, },
      sql = { { 'sqlfluff', }, },
      typescript = { { 'prettierd', }, },
      typescriptreact = { { 'prettierd', }, },
      yaml = { { 'prettierd', }, },
      ['*'] = { 'trim_whitespaces', 'trim_newlines', },
    },
    format_on_save = { timeout_ms = 500, lsp_fallback = true, },
  },
}
