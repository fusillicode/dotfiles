return {
  'dustinblackman/oatmeal.nvim',
  cmd = { 'Oatmeal', },
  keys = { { '<leader>om', mode = 'n', }, },
  opts = {
    backend = 'ollama',
    model = 'codellama:latest',
  },
}
