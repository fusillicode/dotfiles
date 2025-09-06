local M = {}

local buffer = require('nvrim').buffer

function M.open_under_cursor()
  local word = buffer.get_word_under_cursor()
  -- Do not open binary files or "words". Only opens URLs, directories, and text files.
  if not word or word.kind == 'BinaryFile' or word.kind == 'Word' then return end

  vim.fn.jobstart({ 'open', vim.fn.expand(word.value), }, {
    detach = true,
    on_exit = function(_, code, _)
      if code ~= 0 then
        vim.notify('Cannot open ' .. word .. ', error code ' .. code, vim.log.levels.ERROR)
      end
    end,
  })
end

return M
