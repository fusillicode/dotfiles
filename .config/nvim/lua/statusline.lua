-- Thank you ChatGPT
local function current_buffer_path()
  return vim.fn.fnamemodify(
    vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(vim.fn.win_getid(vim.fn.winnr('#')))),
    ':~:.'
  )
end

local M = {}

function M.draw()
  local buffer = vim.fn.bufnr()
  local buffer_errors, buffer_warns, buffer_infos, buffer_hints = 0, 0, 0, 0
  local workspace_errors, workspace_warns, workspace_infos, workspace_hints = 0, 0, 0, 0

  for _, diagnostic in ipairs(vim.diagnostic.get()) do
    if diagnostic.bufnr == buffer then
      if diagnostic.severity == vim.diagnostic.severity.ERROR then
        buffer_errors = buffer_errors + 1
      elseif diagnostic.severity == vim.diagnostic.severity.WARN then
        buffer_warns = buffer_warns + 1
      elseif diagnostic.severity == vim.diagnostic.severity.INFO then
        buffer_infos = buffer_infos + 1
      elseif diagnostic.severity == vim.diagnostic.severity.HINT then
        buffer_hints = buffer_hints + 1
      end
    end
    if diagnostic.severity == vim.diagnostic.severity.ERROR then
      workspace_errors = workspace_errors + 1
    elseif diagnostic.severity == vim.diagnostic.severity.WARN then
      workspace_warns = workspace_warns + 1
    elseif diagnostic.severity == vim.diagnostic.severity.INFO then
      workspace_infos = workspace_infos + 1
    elseif diagnostic.severity == vim.diagnostic.severity.HINT then
      workspace_hints = workspace_hints + 1
    end
  end

  return (buffer_errors ~= 0 and '%#DiagnosticStatusLineError#' .. 'E:' .. buffer_errors .. ' ' or '')
      .. (buffer_warns ~= 0 and '%#DiagnosticStatusLineWarn#' .. 'W:' .. buffer_warns .. ' ' or '')
      .. (buffer_infos ~= 0 and '%#DiagnosticStatusLineInfo#' .. 'I:' .. buffer_infos .. ' ' or '')
      .. (buffer_hints ~= 0 and '%#DiagnosticStatusLineHint#' .. 'H:' .. buffer_hints .. ' ' or '')
      .. '%#StatusLine#'
      -- https://stackoverflow.com/a/45244610
      .. current_buffer_path() .. ' %m %r'
      .. '%='
      .. (workspace_errors ~= 0 and '%#DiagnosticStatusLineError#' .. 'E:' .. workspace_errors .. ' ' or '')
      .. (workspace_warns ~= 0 and '%#DiagnosticStatusLineWarn#' .. 'W:' .. workspace_warns .. ' ' or '')
      .. (workspace_infos ~= 0 and '%#DiagnosticStatusLineInfo#' .. 'I:' .. workspace_infos .. ' ' or '')
      .. (workspace_hints ~= 0 and '%#DiagnosticStatusLineHint#' .. 'H:' .. workspace_hints .. ' ' or '')
end

local function redraw()
  vim.o.statusline = M.draw()
end

vim.api.nvim_create_autocmd({ 'DiagnosticChanged', 'BufEnter', }, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  callback = redraw,
})

return M
