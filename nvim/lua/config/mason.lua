require("mason").setup()
require("mason-lspconfig").setup {
  ensure_installed = { "rust_analyzer", "sumneko_lua" },
}
