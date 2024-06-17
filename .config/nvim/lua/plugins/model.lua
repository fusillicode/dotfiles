local function llama3_header(role)
  return '<|start_header_id|>' .. role .. '<|end_header_id|>'
end

local function llama3_prompt_as(role, prompt)
  return llama3_header(role) .. '\n' .. prompt .. '<|eot_id|>'
end

local function llama3_chat(messages, config, system_prompt)
  local prompt = llama3_prompt_as(
    'system',
    (system_prompt and system_prompt or '') .. (config.system and ('Additionally ' .. config.system) or '')
  )

  for _, msg in ipairs(messages) do
    prompt = prompt .. llama3_prompt_as(msg.role, msg.content)
  end

  prompt = prompt .. llama3_header('assistant')

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

            local prompt = llama3_prompt_as(
                  'system',
                  "You're a Software Engineer who write clear and succinct commits following the Convetional Commits convention."
                )
                .. llama3_prompt_as(
                  'user',
                  'Write a short commit message for the following git diff with conventional commit type in lowercase: '
                  .. '```\n'
                  .. git_diff
                  .. '\n```'
                  .. '<|eot_id|>'
                )
                .. llama3_header('assistant')

            return { prompt = prompt, raw = true, }
          end,
        },
        translate = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.REPLACE,
          builder = function(input)
            local lang = vim.fn.input('Language: ')

            local prompt = llama3_prompt_as(
                  'system',
                  "You're an " .. lang .. ' native speaker who work as a translator.'
                )
                .. llama3_prompt_as(
                  'user',
                  'Translate the following text into ' .. lang .. ' and print ONLY the translation: '
                  .. input
                  .. '<|eot_id|>'
                )
                .. llama3_header('assistant')

            return { prompt = prompt, raw = true, }
          end,
        },
      },
    })
  end,
}
