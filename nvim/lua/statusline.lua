local M = {}

function M.draw()
  local buffer = vim.fn.bufnr()
  local buffer_errors, buffer_warns, buffer_hints, buffer_infos = 0, 0, 0, 0
  local workspace_errors, workspace_warns, workspace_hints, workspace_infos = 0, 0, 0, 0

  for _, diagnostic in ipairs(vim.diagnostic.get()) do
    if diagnostic.bufnr == buffer then
      if diagnostic.severity == vim.diagnostic.severity.ERROR then
        buffer_errors = buffer_errors + 1
      elseif diagnostic.severity == vim.diagnostic.severity.WARN then
        buffer_warns = buffer_warns + 1
      elseif diagnostic.severity == vim.diagnostic.severity.HINT then
        buffer_hints = buffer_hints + 1
      elseif diagnostic.severity == vim.diagnostic.severity.INFO then
        buffer_infos = buffer_infos + 1
      end
    end
    if diagnostic.severity == vim.diagnostic.severity.ERROR then
      workspace_errors = workspace_errors + 1
    elseif diagnostic.severity == vim.diagnostic.severity.WARN then
      workspace_warns = workspace_warns + 1
    elseif diagnostic.severity == vim.diagnostic.severity.HINT then
      workspace_hints = workspace_hints + 1
    elseif diagnostic.severity == vim.diagnostic.severity.INFO then
      workspace_infos = workspace_infos + 1
    end
  end

  return (buffer_errors ~= 0 and '%#DiagnosticError#' .. 'E:' .. buffer_errors .. ' ' or '')
      .. (buffer_warns ~= 0 and '%#DiagnosticWarn#' .. 'W:' .. buffer_warns .. ' ' or '')
      .. (buffer_hints ~= 0 and '%#DiagnosticHint#' .. 'H:' .. buffer_hints .. ' ' or '')
      .. (buffer_infos ~= 0 and '%#DiagnosticInfo#' .. 'I:' .. buffer_infos .. ' ' or '')
      .. '%#StatusLine#'
      .. ' %f %m %r'
      .. '%='
      .. require('lsp-progress').progress()
      .. ' '
      .. (workspace_errors ~= 0 and '%#DiagnosticError#' .. 'E:' .. workspace_errors .. ' ' or '')
      .. (workspace_warns ~= 0 and '%#DiagnosticWarn#' .. 'W:' .. workspace_warns .. ' ' or '')
      .. (workspace_hints ~= 0 and '%#DiagnosticHint#' .. 'H:' .. workspace_hints .. ' ' or '')
      .. (workspace_infos ~= 0 and '%#DiagnosticInfo#' .. 'I:' .. workspace_infos .. ' ' or '')
end

vim.api.nvim_create_autocmd({ 'DiagnosticChanged', 'BufEnter', }, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  callback = function(_) vim.o.statusline = M.draw() end,
})

vim.api.nvim_create_autocmd({ 'User', }, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  pattern = 'LspProgressStatusUpdated',
  callback = function(_) vim.o.statusline = M.draw() end,
})

return M
