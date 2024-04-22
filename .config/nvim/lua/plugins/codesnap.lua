return {
  'mistricky/codesnap.nvim',
  cmd = 'CodeSnap',
  build = 'make',
  config = function()
    require('codesnap').setup({
      bg_color = '#000000',
      watermark = '',
    })
  end,
}
