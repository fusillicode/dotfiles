local function vale_config()
  return {
    cmd = 'vale',
    stdin = true,
    args = { '--no-exit', '--output', 'JSON', },
    stream = 'stdout',
    ignore_exitcode = true,
    parser = function(output, _)
      local diagnostics = {}
      local decoded = vim.fn.json_decode(output)

      if not decoded then return diagnostics end

      for _, diagnostic in ipairs(decoded['stdin.txt']) do
        table.insert(diagnostics, {
          lnum = diagnostic.Line - 1,
          col = diagnostic.Span[1] - 1,
          end_lnum = diagnostic.Line - 1,
          end_col = diagnostic.Span[2],
          severity = vim.diagnostic.severity.HINT,
          message = diagnostic.Message,
          source = 'vale',
        })
      end

      return diagnostics
    end,
  }
end

return {
  'mfussenegger/nvim-lint',
  config = function()
    local lint = require('lint')

    lint.linters_by_ft = {
      dockerfile = { 'hadolint', },
      elixir = { 'credo', },
      javascript = { 'eslint_d', },
      markdown = { 'vale', },
      sql = { 'sqruff', },
      typescript = { 'eslint_d', },
      typescriptreact = { 'eslint_d', },
    }

    lint.linters.vale = vale_config()

    vim.api.nvim_create_autocmd('BufWritePost', {
      group = vim.api.nvim_create_augroup('FormatBufferWithNvimLint', { clear = true, }),
      callback = function() lint.try_lint() end,
    })
  end,
}
