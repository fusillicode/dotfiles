return {
  'lvimuser/lsp-inlayhints.nvim',
  event = 'LspAttach',
  config = function()
    vim.api.nvim_create_autocmd('LspAttach', {
      group = vim.api.nvim_create_augroup('LspAttachInlayHints', { clear = true, }),
      callback = function(args)
        if not (args.data and args.data.client_id) then
          return
        end

        local client = vim.lsp.get_client_by_id(args.data.client_id)
        -- https://vinnymeller.com/posts/neovim_nightly_inlay_hints/#globally
        if client.server_capabilities.inlayHintProvider then
          vim.lsp.inlay_hint.enable(args.buf, true)
        end
        require('lsp-inlayhints').on_attach(client, args.buf)
      end,
    })

    require('lsp-inlayhints').setup({
      inlay_hints = {
        parameter_hints = {
          prefix = '',
          remove_colon_start = true,
        },
        type_hints = {
          remove_colon_start = true,
          remove_colon_end = true,
        },
      },
    })
  end,
}
