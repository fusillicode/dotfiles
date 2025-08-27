return {
  'kndndrj/nvim-dbee',
  cmd = { 'Dbee', },
  dependencies = { 'MunifTanjim/nui.nvim', },
  build = function() require('dbee').install() end,
  config = function()
    require('dbee').setup({
      sources = {
        require('dbee.sources').FileSource:new(
          vim.env.HOME .. '/.local/state/nvim/dbee/conns.json'
        ),
      },
    })
  end,
}
