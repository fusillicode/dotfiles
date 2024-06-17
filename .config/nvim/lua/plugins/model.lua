local function llama3_chat(messages, config, system_prompt)
  local prompt = '<|start_header_id|>system<|end_header_id|>\n'
      .. (system_prompt and system_prompt or '')
      .. (config.system and ('Additionally ' .. config.system) or '')
      .. '<|eot_id|>'

  for _, msg in ipairs(messages) do
    prompt = prompt
        .. '<|start_header_id|>'
        .. (msg.role == 'user' and 'user' or 'assistant')
        .. '<|end_header_id|>\n'
        .. msg.content
        .. '<|eot_id|>'
  end

  prompt = prompt .. '<|start_header_id|>assistant<|end_header_id|>'

  return { prompt = prompt, raw = true, }
end

local function llama3_chat_config(system_prompt)
  return {
    provider = require('model.providers.ollama'),
    params = { model = 'llama3:latest', },
    create = function(input, context) return context.selection and input or '' end,
    run = function(messages, config)
      return llama3_chat(messages, config, system_prompt)
    end,
  }
end

return {
  'gsuuon/model.nvim',
  keys = { '<leader>[', },
  cmd = { 'M', 'Model', 'Mchat', },
  init = function()
    vim.filetype.add({
      extension = {
        mchat = 'mchat',
      },
    })
  end,
  ft = 'mchat',
  config = function()
    require('keymaps').model()

    require('model').setup({
      chats = {
        concise = llama3_chat_config("Use less words as possible and don't hallucinate!"),
        friendly = llama3_chat_config('Be kind and friendly'),
        bool = llama3_chat_config('Answer ONLY with yes or no'),
      },
      prompts = {
        commit = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.INSERT,
          builder = function()
            local git_diff = vim.fn.system({ 'git', 'diff', '--staged', })

            if git_diff == '' then
              return
            end

            local prompt = '<|start_header_id|>system<|end_header_id|>\n'
                .. 'Use Convetional Commit with lowercase type and write ONLY the short commit message for the following git diff: '
                .. '```\n'
                .. git_diff
                .. '\n```'
                .. '<|eot_id|>'
                .. '<|start_header_id|>assistant<|end_header_id|>'

            return { prompt = prompt, }
          end,
        },
      },
    })
  end,
}
