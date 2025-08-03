return {
  {
    'CopilotC-Nvim/CopilotChat.nvim',
    dependencies = {
      { 'nvim-lua/plenary.nvim', branch = 'master', },
    },
    build = 'make tiktoken',
    config = function()
      require('CopilotChat').setup({})
    end,
  },
}
