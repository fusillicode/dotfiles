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
  local unpack = table.unpack or unpack

  local _, start_ln, start_col = unpack(vim.fn.getpos('v'))
  local _, end_ln, end_col = unpack(vim.fn.getpos('.'))

  start_ln, end_ln = math.min(start_ln, end_ln), math.max(start_ln, end_ln)
  start_col, end_col = math.min(start_col, end_col), math.max(start_col, end_col)

  return vim.api.nvim_buf_get_text(0, start_ln - 1, start_col - 1, end_ln - 1, end_col, {})[1]
end

return M
