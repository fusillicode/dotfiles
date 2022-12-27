require("auto-save").setup {
  enabled = true,
  execution_message = {
    message = function()
      return ("Saved at " .. vim.fn.strftime("%H:%M:%S"))
    end,
    cleaning_interval = 1250,
  },
  trigger_events = {"InsertLeave", "TextChanged"},
  condition = function(buf)
    local fn = vim.fn
    local utils = require("auto-save.utils.data")

    if
      fn.getbufvar(buf, "&modifiable") == 1 and
      utils.not_in(fn.getbufvar(buf, "&filetype"), {}) then
      return true
    end
    return false
  end,
  write_all_buffers = false,
  debounce_delay = 135,
}
