require('trim').setup({
  disable = {"markdown"},

  patterns = {
    [[%s/\(\n\n\)\n\+/\1/]],
  },
})
