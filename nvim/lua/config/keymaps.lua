local opts = { noremap = true, silent = true }
vim.keymap.set('v', '<', '<gv', opts)
vim.keymap.set('v', '>', '>gv', opts)
vim.keymap.set("n", "<leader>e", ":Lexplore <cr>", opts)
