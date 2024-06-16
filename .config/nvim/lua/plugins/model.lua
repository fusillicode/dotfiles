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

return {
  'gsuuon/model.nvim',
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
    local ollama = require('model.providers.ollama')

    require('model').setup({
      chats = {
        llama3 = {
          provider = ollama,
          params = { model = 'llama3:latest', },
          create = function(input, context)
            return context.selection and input or ''
          end,
          run = function(messages, config)
            return llama3_chat(
              messages,
              config,
              "Use less words as possible and don't hallucinate!"
            )
          end,
        },
      },
    })
  end,
}
