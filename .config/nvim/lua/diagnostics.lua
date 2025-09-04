local M = {}

function M.setup(nvrim)
  vim.diagnostic.config({
    float = {
      anchor_bias = 'above',
      border = nvrim.get_style_opts().window.border,
      focusable = true,
      format = nvrim.format_diagnostic,
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
    -- NOTE: switch to this line if `nvrim.filter_diagnostics(diagnostics)` misbehave
    -- diag_set(namespace, bufnr, diagnostics, opts)
    diag_set(
      namespace,
      bufnr,
      nvrim.sort_diagnostics(
        nvrim.filter_diagnostics(vim.api.nvim_buf_get_name(bufnr), diagnostics)
      ),
      opts
    )
  end
end

return M
