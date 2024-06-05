return {
  'David-Kunz/gen.nvim',
  config = function()
    local gen = require('gen')
    require('keymaps').gen()

    for k, _ in pairs(gen.prompts) do
      gen.prompts[k]['replace'] = false
    end

    gen.setup({
      model = 'llama3:latest',
      display_mode = 'split',
      show_prompt = true,
      show_model = true,
    })
  end,
}
