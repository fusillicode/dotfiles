return {
  'lvimuser/lsp-inlayhints.nvim',
  event = 'LspAttach',
  config = function()
    vim.api.nvim_create_augroup('LspAttachInlayHints', {})
    vim.api.nvim_create_autocmd('LspAttach', {
      group = 'LspAttachInlayHints',
      callback = function(args)
        if not (args.data and args.data.client_id) then
          return
        end

        local client = vim.lsp.get_client_by_id(args.data.client_id)
        require('lsp-inlayhints').on_attach(client, args.buf)
      end,
    })

    require('lsp-inlayhints').setup({
      inlay_hints = {
        parameter_hints = {
          prefix = '',
        }
      }
    })
  end,
}