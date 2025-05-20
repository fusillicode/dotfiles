return {
  'stevearc/conform.nvim',
  event = 'BufWritePre',
  cmd = { 'ConformInfo', 'ConformAt', },
  config = function()
    local conform = require('conform')
    -- https://github.com/stevearc/conform.nvim/blob/master/doc/recipes.md#automatically-run-slow-formatters-async
    local slow_format_filetypes = {}

    conform.setup({
      formatters = {
        sqruff = {
          prepend_args = { '--config', os.getenv('HOME') .. '/data/dev/dotfiles/dotfiles/.sqruff', },
          require_cwd = false,
        },
      },
      formatters_by_ft = {
        css = { 'prettierd', },
        graphql = { 'prettierd', },
        javascript = { 'prettierd', },
        markdown = { 'prettierd', },
        sql = { 'sqruff', },
        typescript = { 'prettierd', },
        typescriptreact = { 'prettierd', },
        yaml = { 'prettierd', },
        ['*'] = { 'trim_whitespace', 'trim_newlines', },
      },
      -- https://github.com/stevearc/conform.nvim/blob/master/doc/recipes.md#automatically-run-slow-formatters-async
      format_on_save = function(bufnr)
        if slow_format_filetypes[vim.bo[bufnr].filetype] then return end

        local function on_format(err)
          if err and err:match('timeout$') then
            slow_format_filetypes[vim.bo[bufnr].filetype] = true
          end
        end

        return { timeout_ms = 500, lsp_fallback = true, }, on_format
      end,
      -- https://github.com/stevearc/conform.nvim/blob/master/doc/recipes.md#automatically-run-slow-formatters-async
      format_after_save = function(bufnr)
        if not slow_format_filetypes[vim.bo[bufnr].filetype] then return end
        return { lsp_fallback = true, }
      end,
      -- https://github.com/stevearc/conform.nvim/blob/master/doc/recipes.md#lazy-loading-with-lazynvim
      init = function()
        vim.o.formatexpr = "v:lua.require'conform'.formatexpr()"
      end,
    })

    -- https://github.com/stevearc/conform.nvim/blob/master/doc/recipes.md#format-command
    vim.api.nvim_create_user_command('Format', function(args)
      local range = nil
      if args.count ~= -1 then
        local end_line = vim.api.nvim_buf_get_lines(0, args.line2 - 1, args.line2, true)[1]
        range = {
          start = { args.line1, 0, },
          ['end'] = { args.line2, end_line:len(), },
        }
      end
      conform.format({ async = true, lsp_format = 'fallback', range = range, })
    end, { range = true, })
  end,
}
