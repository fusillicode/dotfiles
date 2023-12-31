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

M.visual_mode = M.new_set({ 'v', 'V', 'CTRL-V', })

function M.esc_without_jumps()
  return (M.visual_mode[vim.api.nvim_get_mode()['mode']]
        and ":<c-u>'" .. (vim.fn.line('.') < vim.fn.line('v') and '<' or '>') .. '<cr>'
        or '')
      .. ':<cr>:noh<cr>:echo""<cr>'
end

return M
