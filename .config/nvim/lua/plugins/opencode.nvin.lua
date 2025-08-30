return {
  'NickvanDyke/opencode.nvim',
  keys = {
    -- Recommended keymaps
    { '<leader>oA', function() require('opencode').ask() end,                                     desc = 'Ask opencode', },
    { '<leader>oa', function() require('opencode').ask('@selection: ') end,                       desc = 'Ask opencode about selection', mode = 'v', },
    { '<leader>on', function() require('opencode').command('session_new') end,                    desc = 'New session', },
    { '<leader>oy', function() require('opencode').command('messages_copy') end,                  desc = 'Copy last message', },
    { '<leader>op', function() require('opencode').select_prompt() end,                           desc = 'Select prompt',                mode = { 'n', 'v', }, },
    { '<leader>oe', function() require('opencode').prompt('Explain @cursor and its context') end, desc = 'Explain code near cursor', },
  },
  config = function()
    require('opencode').setup({})
  end,
}
