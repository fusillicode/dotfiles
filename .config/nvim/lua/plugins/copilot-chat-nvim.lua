return {
  'CopilotC-Nvim/CopilotChat.nvim',
  keys = { '<leader>co', },
  dependencies = { { 'nvim-lua/plenary.nvim', branch = 'master', }, },
  build = 'make tiktoken',
  config = function()
    local copilot_chat = require('CopilotChat')

    copilot_chat.setup({
      auto_follow_cursor = false,
      highlight_headers = false,
      insert_at_end = true,
      separator = '---',
      error_header = '> [!ERROR] Error',
      selection = function(source)
        return require('CopilotChat.select').visual(source) or
            require('CopilotChat.select').buffer(source)
      end,
    })

    require('keymaps').copilot_chat(copilot_chat)

    vim.api.nvim_create_autocmd('BufEnter', {
      pattern = 'copilot-*',
      callback = function()
        vim.opt_local.relativenumber = false
        vim.opt_local.number = false
        vim.opt_local.conceallevel = 0
      end,
    })
  end,
}
