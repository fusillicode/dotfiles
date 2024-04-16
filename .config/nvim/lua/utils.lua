local M = {}

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

M.normal_esc = ':noh<cr>:echo""<cr>'

function M.visual_esc()
  return ":<c-u>'" .. (vim.fn.line('.') < vim.fn.line('v') and '<' or '>') .. '<cr>' .. M.normal_esc
end

-- https://github.com/nvim-telescope/telescope-live-grep-args.nvim/blob/731a046da7dd3adff9de871a42f9b7fb85f60f47/lua/telescope-live-grep-args/shortcuts.lua#L8-L17
function M.get_visual_selection()
  ---@diagnostic disable-next-line: deprecated
  table.unpack = table.unpack or unpack

  local _, ls, cs = table.unpack(vim.fn.getpos('v'))
  local _, le, ce = table.unpack(vim.fn.getpos('.'))

  ls, le = math.min(ls, le), math.max(ls, le)
  cs, ce = math.min(cs, ce), math.max(cs, ce)

  return vim.api.nvim_buf_get_text(0, ls - 1, cs - 1, le - 1, ce, {})[1]
end

return M
