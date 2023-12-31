local M = {}

function M.keymap_set(modes, lhs, rhs, opts)
  vim.keymap.set(modes, lhs, rhs, vim.tbl_extend('force', { silent = true, }, opts or {}))
end

function M.dbg(foo)
  print(vim.inspect(foo))
  return foo
end

function M.set_diff(s1, s2)
  local diff = {}
  for k, v in pairs(s1) do if s2[k] == nil then diff[k] = v end end
  return diff
end

function M.new_set(table)
  local set = {}
  for _, v in ipairs(table) do set[v] = true end
  return set
end

function M.no_jumping_esc()
  local mode = vim.api.nvim_get_mode()['mode']
  local goto_mark = ''
  if mode == 'v' or mode == 'V' or mode == 'CTRL-V' then
    goto_mark = vim.fn.line('.') < vim.fn.line('v') and ":<c-u>'<<cr>" or ":<c-u>'><cr>"
  end
  return goto_mark .. ':<cr>:noh<cr>:echo""<cr>'
end

return M
