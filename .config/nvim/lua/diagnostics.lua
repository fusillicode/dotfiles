local M = {}

function M.setup(nvrim)
  vim.diagnostic.config(nvrim.diagnostics.config)
  local diag_set = vim.diagnostic.set
  vim.diagnostic.set = function(namespace, bufnr, diagnostics, opts)
    -- NOTE: enable this line to understand what's happening with diagnostics
    require('utils').log(diagnostics)
    -- NOTE: switch to this line if `nvrim.filter_diagnostics(diagnostics)` misbehave
    -- diag_set(namespace, bufnr, diagnostics, opts)
    diag_set(
      namespace,
      bufnr,
      nvrim.diagnostics.sort(
        nvrim.diagnostics.filter(vim.api.nvim_buf_get_name(bufnr), diagnostics)
      ),
      opts
    )
  end
end

return M
