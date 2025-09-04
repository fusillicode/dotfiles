local nvrim = require('nvrim')

local M = {}

function M.open_under_cursor()
  local word = vim.fn.expand(nvrim.get_word_under_cursor())
  if word == nil or word == '' then return end
  vim.fn.jobstart({ 'open', word, }, {
    detach = true,
    on_exit = function(_, code, _)
      if code ~= 0 then
        vim.notify('Cannot open ' .. word .. ', error code ' .. code, vim.log.levels.ERROR)
      end
    end,
  })
end

return M
