local M = {}

M.buffer = require('nvrim').buffer

function M.open_under_cursor()
  local word = vim.fn.expand(M.buffer.get_word_under_cursor())
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
