return {
  'CopilotC-Nvim/CopilotChat.nvim',
  keys = { '<leader>co', },
  dependencies = { { 'nvim-lua/plenary.nvim', branch = 'master', }, },
  build = 'make tiktoken',
  config = function()
    local co_chat = require('CopilotChat')
    local co_chat_select = require('CopilotChat.select')

    co_chat.setup({
      auto_follow_cursor = false,
      insert_at_end = true,
      error_header = '> [!ERROR] ❌',
      headers = {
        user = '🤓 You: ',
        assistant = '🤖 AI Assistant: ',
        tool = '🔧 Tool: ',
      },
      show_folds = false,
      show_help = false,
      selection = function(source)
        return co_chat_select.visual(source) or co_chat_select.buffer(source)
      end,
    })

    require('keymaps').copilot_chat(co_chat)

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
