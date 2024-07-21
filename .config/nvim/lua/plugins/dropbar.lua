return {
  'Bekaboo/dropbar.nvim',
  event = 'BufEnter',
  dependencies = { 'nvim-telescope/telescope-fzf-native.nvim', },
  config = function()
    local utils = require('dropbar.utils')
    local api = require('dropbar.api')

    require('keymaps').dropbar(api, utils)

    require('dropbar').setup({
      icons = {
        ui = {
          bar = {
            separator = ' ',
          },
        },
      },
      menu = {
        keymaps = {
          ['h'] = function()
            local menu = utils.menu.get_current()
            if not menu then return end

            if menu.prev_menu then
              menu:close()
              return
            end

            for _, comp in ipairs(utils.bar.get({ win = menu.prev_win, }).components) do
              if comp.menu then
                menu:close()
                api.pick(comp.bar_idx - 1)
                return
              end
            end
          end,
          ['l'] = function()
            local menu = utils.menu.get_current()
            if not menu then return end

            local cursor = vim.api.nvim_win_get_cursor(menu.win)
            local component = menu.entries[cursor[1]]:first_clickable(cursor[2])
            if component then menu:click_on(component, nil, 1, 'l') end
          end,
        },
      },
    })
  end,
}
