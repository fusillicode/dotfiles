return {
  'stevearc/conform.nvim',
  event = 'BufWritePre',
  cmd = { 'ConformInfo', 'ConformAt', },
  config = function()
    local conform = require('conform')
    local nvrim = require('nvrim')
    -- https://github.com/stevearc/conform.nvim/blob/master/doc/recipes.md#automatically-run-slow-formatters-async
    local slow_format_filetypes = {}

    conform.setup({
      formatters = {
        qj = { command = 'qj', args = { '.', }, stdin = true, },
        sqruff = {
          prepend_args = { '--config', os.getenv('HOME') .. '/data/dev/dotfiles/dotfiles/.sqruff', },
          require_cwd = false,
        },
      },
      formatters_by_ft = {
        css = { 'prettierd', },
        graphql = { 'prettierd', },
        javascript = { 'prettierd', },
        json = { 'qj', },
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

    local function formatter_names(args, lines)
      local explicit = args.fargs[1]
      if explicit and explicit ~= '' then
        return conform.formatters_by_ft[explicit] or { explicit, }
      end

      if conform.formatters_by_ft[vim.bo.filetype] then return conform.formatters_by_ft[vim.bo.filetype] end
      if pcall(vim.json.decode, table.concat(lines, '\n')) then return conform.formatters_by_ft.json end
      return conform.formatters_by_ft['*']
    end

    local function conform_at(args)
      local selection = nvrim.buffer.get_selection_for_ex_range(args.line1, args.line2)
      if not selection then return end
      conform.format_lines(formatter_names(args, selection.lines), selection.lines, { async = true, }, function(err, formatted_lines)
        if err then
          vim.notify(err.message or err, vim.log.levels.ERROR)
          return
        end
        vim.api.nvim_buf_set_text(0, selection.start[1], selection.start[2], selection['end'][1], selection['end'][2], formatted_lines)
      end)
    end

    local function format_range(args)
      if args.count ~= -1 then
        conform_at(args)
        return
      end

      conform.format({ async = true, lsp_format = 'fallback', })
    end

    vim.api.nvim_create_user_command('ConformAt', format_range, { nargs = '?', range = true, })
    vim.api.nvim_create_user_command('Format', format_range, { nargs = '?', range = true, })
  end,
}
