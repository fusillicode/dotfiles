local Llama3 = {
  header = function(role) return '<|start_header_id|>' .. role .. '<|end_header_id|>' end,
  prompt_as = function(self, role, prompt) return self.header(role) .. '\n' .. prompt .. '<|eot_id|>' end,
  chat = function(self, messages, config, system_prompt)
    local prompt = self:prompt_as(
      'system',
      (system_prompt and system_prompt or '') .. (config.system and ('Additionally ' .. config.system) or '')
    )

    for _, msg in ipairs(messages) do prompt = prompt .. self:prompt_as(msg.role, msg.content) end

    return { prompt = prompt .. self.header('assistant'), raw = true, }
  end,
  chat_config = function(self, system_prompt)
    return {
      provider = require('model.providers.ollama'),
      params = { model = 'llama3:latest', },
      create = function(input, context) return context.selection and input or '' end,
      run = function(messages, config) return self:chat(messages, config, system_prompt) end,
    }
  end,
}

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
        concise = Llama3:chat_config("Use less words as possible and don't hallucinate!"),
        friendly = Llama3:chat_config('Be kind and friendly'),
        bool = Llama3:chat_config('Answer only with yes or no'),
      },
      prompts = {
        commit = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.INSERT_OR_REPLACE,
          builder = function()
            local git_diff = vim.fn.system({ 'git', 'diff', '--staged', })

            if git_diff == '' then error('Empty git diff') end

            local prompt = Llama3:prompt_as(
                  'system',
                  "You're a Software Engineer who write clear and succinct commits following the Convetional Commits convention."
                )
                .. Llama3:prompt_as(
                  'user',
                  'Write just a commit message for the following git diff with conventional commit type in lowercase: '
                  .. '```\n' .. git_diff .. '\n```'
                )
                .. Llama3.header('assistant')

            return { prompt = prompt, raw = true, }
          end,
        },
        translate = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.INSERT_OR_REPLACE,
          builder = function(input)
            local lang = vim.fn.input('Language: ')

            if lang == '' then error('No language supplied') end

            local prompt = Llama3:prompt_as(
                  'system',
                  "You're an " .. lang .. ' native speaker who work as a translator.'
                )
                .. Llama3:prompt_as(
                  'user',
                  'Translate the following text into ' .. lang .. ' and output only the translation: ' .. input
                )
                .. Llama3.header('assistant')

            return { prompt = prompt, raw = true, }
          end,
        },
        refactor = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.INSERT_OR_REPLACE,
          builder = function(input)
            local lang = vim.fn.input('Language: ')

            if lang == '' then error('No language supplied') end

            local prompt = Llama3:prompt_as('system', "You're a Software Engineer expert in " .. lang)
                .. Llama3:prompt_as(
                  'user',
                  'Refactor the following code: ' .. '```\n' .. input .. '\n```'
                )
                .. Llama3.header('assistant')

            return { prompt = prompt, raw = true, }
          end,
        },
      },
    })
  end,
}
