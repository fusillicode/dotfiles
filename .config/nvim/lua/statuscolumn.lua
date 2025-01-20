local M = {}

local function format_extmark(extmark)
  return (extmark and ('%#' .. extmark.sign_hl_group .. '#' .. vim.trim(extmark.sign_text) .. '%*') or ' ')
end

function M.draw(current_lnum)
  local line_signs = vim.api.nvim_buf_get_extmarks(
    vim.fn.bufnr(), -1, { current_lnum - 1, 0, }, { current_lnum - 1, -1, },
    { type = 'sign', details = true, overlap = false, }
  )
  -- require('utils').dbg(line_signs)
  -- require('rua').draw_statuscolumn(line_signs)

  local git_sign, error_sign, warn_sign, hint_sign, info_sign, ok_sign
  for _, sign in ipairs(line_signs) do
    local sign_details = sign[4]

    if sign_details.sign_hl_group:sub(1, 8) == 'GitSigns' then
      git_sign = sign_details
    elseif sign_details.sign_hl_group == 'DiagnosticSignError' then
      error_sign = sign_details
    elseif sign_details.sign_hl_group == 'DiagnosticSignWarn' then
      warn_sign = sign_details
    elseif sign_details.sign_hl_group == 'DiagnosticSignInfo' then
      info_sign = sign_details
    elseif sign_details.sign_hl_group == 'DiagnosticSignHint' then
      hint_sign = sign_details
    elseif sign_details.sign_hl_group == 'DiagnosticSignOk' then
      ok_sign = sign_details
    end
  end

  return format_extmark(error_sign or warn_sign or info_sign or hint_sign or ok_sign)
      .. format_extmark(git_sign)
      .. ' %=%{v:lnum} '
end

return M
