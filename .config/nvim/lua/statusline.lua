local function get_git_branch()
  local handle = io.popen('git rev-parse --abbrev-ref HEAD 2>/dev/null')
  if not handle then return '' end

  local result = handle:read('*a')
  handle:close()
  return result:gsub('%s+', '')
end

local function get_git_diff()
  local handle = io.popen('git diff --shortstat 2>/dev/null')
  if not handle then return '' end

  local result = handle:read('*a')
  handle:close()
  return result:match('(%d+)%s*[^%d]*,%s*(%d+)%s*[^%d]*,%s*(%d+)')
end

local function get_git_sha()
  local handle = io.popen('git rev-parse --short HEAD 2>/dev/null')
  if not handle then return '' end

  local result = handle:read('*a')
  handle:close()
  return result:gsub('%s+', '')
end

local M = {}

local status_line_hl_group = vim.api.nvim_get_hl(0, { name = 'StatusLine', })
for _, diagnostic_hl_group in ipairs({
  'DiagnosticError',
  'DiagnosticWarn',
  'DiagnosticInfo',
  'DiagnosticHint',
  'DiagnosticOk',
}) do
  vim.api.nvim_set_hl(0, diagnostic_hl_group .. 'StatusLine',
    vim.tbl_extend(
      'force',
      status_line_hl_group,
      { fg = vim.api.nvim_get_hl(0, { name = diagnostic_hl_group, }).fg, }
    )
  )
end

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

  local changes, inserts, deletes = get_git_diff()
  local git_sha = get_git_sha()

  return (buffer_errors ~= 0 and '%#DiagnosticErrorStatusLine#' .. 'E:' .. buffer_errors .. ' ' or '')
      .. (buffer_warns ~= 0 and '%#DiagnosticWarnStatusLine#' .. 'W:' .. buffer_warns .. ' ' or '')
      .. (buffer_infos ~= 0 and '%#DiagnosticInfoStatusLine#' .. 'I:' .. buffer_infos .. ' ' or '')
      .. (buffer_hints ~= 0 and '%#DiagnosticHintStatusLine#' .. 'H:' .. buffer_hints .. ' ' or '')
      .. '%#StatusLine#'
      .. (get_git_branch() or '')
      .. (git_sha and ' ' .. git_sha or '')
      .. (changes and ' ~' .. changes or '')
      .. (inserts and ' +' .. inserts or '')
      .. (deletes and ' -' .. deletes or '')
      .. '%#StatusLine#'
      .. ' %m %r' .. '%='
      .. (workspace_errors ~= 0 and '%#DiagnosticErrorStatusLine#' .. 'E:' .. workspace_errors .. ' ' or '')
      .. (workspace_warns ~= 0 and '%#DiagnosticWarnStatusLine#' .. 'W:' .. workspace_warns .. ' ' or '')
      .. (workspace_infos ~= 0 and '%#DiagnosticInfoStatusLine#' .. 'I:' .. workspace_infos .. ' ' or '')
      .. (workspace_hints ~= 0 and '%#DiagnosticHintStatusLine#' .. 'H:' .. workspace_hints .. ' ' or '')
end

local function redraw()
  vim.o.statusline = M.draw()
end

vim.api.nvim_create_autocmd({ 'DiagnosticChanged', 'BufEnter', }, {
  group = vim.api.nvim_create_augroup('StatusLine', {}),
  callback = redraw,
})

return M
