vim.diagnostic.config {
  float = {
    anchor_bias = 'above',
    border = 'rounded',
    focusable = true,
    format = function(diagnostic)
      local message =
          (
            vim.tbl_get(diagnostic, 'user_data', 'lsp', 'data', 'rendered') or
            vim.tbl_get(diagnostic, 'user_data', 'lsp', 'message') or
            ''
          ):gsub('%.$', '')
      if message == '' then return end

      local lsp_data = vim.tbl_get(diagnostic, 'user_data', 'lsp')
      if not lsp_data then return end

      local source_code_tbl = vim.tbl_filter(function(x) return x ~= nil and x ~= '' end, {
        lsp_data.source and lsp_data.source:gsub('%.$', '') or nil,
        lsp_data.code and lsp_data.code:gsub('%.$', '') or nil,
      })
      local source_code = table.concat(source_code_tbl, ': ')

      return '▶ ' .. message .. (source_code ~= '' and ' [' .. source_code .. ']' or '')
    end,
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
