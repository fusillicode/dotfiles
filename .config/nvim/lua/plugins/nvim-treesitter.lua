return {
  'nvim-treesitter/nvim-treesitter',
  branch = 'main',
  lazy = false,
  dependencies = {
    { 'nvim-treesitter/nvim-treesitter-textobjects', branch = 'main' },
  },
  build = ':TSUpdate',
  config = function()
    require('nvim-treesitter').setup()

    -- ensure_installed equivalent
    require('nvim-treesitter.install').install({
      'bash',
      'comment',
      'css',
      'diff',
      'dockerfile',
      'git_config',
      'git_rebase',
      'gitattributes',
      'gitcommit',
      'gitignore',
      'graphql',
      'html',
      'javascript',
      'json',
      'kdl',
      'lua',
      'make',
      'markdown',
      'markdown_inline',
      'mermaid',
      'python',
      'regex',
      'rust',
      'sql',
      'textproto',
      'toml',
      'typescript',
      'vim',
      'vimdoc',
      'xml',
      'yaml',
    })

    local sel_stack = {}
    local esc = vim.api.nvim_replace_termcodes('<Esc>', true, false, true)

    local function ts_select(node)
      local sr, sc, er, ec = node:range()
      if vim.api.nvim_get_mode().mode ~= 'n' then
        vim.api.nvim_feedkeys(esc, 'nx', false)
      end
      vim.api.nvim_win_set_cursor(0, { sr + 1, sc })
      vim.api.nvim_feedkeys('v', 'nx', false)
      -- node:range() end is exclusive; when ec==0 the node ends at the start of
      -- line er, so the last actual character is at the end of line er-1.
      local end_row, end_col
      if ec == 0 then
        end_row = er
        end_col = #(vim.api.nvim_buf_get_lines(0, er - 1, er, false)[1] or '')
      else
        end_row = er + 1
        end_col = ec - 1
      end
      vim.api.nvim_win_set_cursor(0, { end_row, end_col })
    end

    vim.keymap.set('n', '<cr>', function()
      local node = vim.treesitter.get_node()
      if not node then return end
      sel_stack = { node }
      ts_select(node)
    end, { desc = 'Treesitter: init selection' })

    vim.keymap.set('x', '<cr>', function()
      local last = sel_stack[#sel_stack]
      if not last then
        local node = vim.treesitter.get_node()
        if not node then return end
        sel_stack = { node }
        ts_select(node)
        return
      end
      local parent = last:parent()
      if not parent then return end
      table.insert(sel_stack, parent)
      ts_select(parent)
    end, { desc = 'Treesitter: expand selection' })

    vim.keymap.set('x', '<s-cr>', function()
      if #sel_stack <= 1 then return end
      table.remove(sel_stack)
      ts_select(sel_stack[#sel_stack])
    end, { desc = 'Treesitter: shrink selection' })
  end,
}
