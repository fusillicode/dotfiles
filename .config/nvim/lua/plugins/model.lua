local Llama3 = {
  header = function(role) return '<|start_header_id|>' .. role .. '<|end_header_id|>' end,
  trigger_response = function(self) return self.header('assistant') end,
  prompt_as = function(self, role, prompt) return self.header(role) .. '\n' .. prompt .. '<|eot_id|>' end,
  user_prompt = function(self, prompt) return self:prompt_as('user', prompt) end,
  system_prompt = function(self, prompt) return self:prompt_as('system', prompt) end,
  code_prompt = function(code, lang) return '\n```' .. (lang and lang or '') .. '\n' .. code .. '\n```' end,
  chat = function(self, system_prompt)
    return {
      provider = require('model.providers.ollama'),
      params = { model = 'llama3:latest', },
      create = function(input, context) return context.selection and input or '' end,
      run = function(messages, config) return self:run_chat(messages, config, system_prompt) end,
    }
  end,
  run_chat = function(self, messages, config, system_prompt)
    local prompt = self:prompt_as(
      'system',
      (system_prompt and system_prompt or '') .. (config.system and ('Additionally ' .. config.system) or '')
    )

    for _, msg in ipairs(messages) do prompt = prompt .. self:prompt_as(msg.role, msg.content) end

    return { prompt = prompt .. self.header('assistant'), raw = true, }
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
        concise = Llama3:chat("Use less words as possible and don't hallucinate!"),
        friendly = Llama3:chat('Be kind and friendly'),
        bool = Llama3:chat('Answer only with yes or no'),
      },
      prompts = {
        means = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.BUFFER,
          builder = function(text)
            local prompt = Llama3:system_prompt("You're an expert linguist in all lanuages")
                .. Llama3:user_prompt('Explain in a concise but precise way what does this means: ' .. text)
                .. Llama3:trigger_response()

            return { prompt = prompt, raw = true, }
          end,
        },
        translate = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.INSERT_OR_REPLACE,
          builder = function(text)
            local lang = vim.fn.input('Language: ')

            if lang == '' then error('No language supplied') end

            local prompt = Llama3:system_prompt("You're an " .. lang .. ' native speaker who work as a translator.')
                .. Llama3:user_prompt(
                  'Translate the following text into ' .. lang .. ' and output only the translation: ' .. text
                )
                .. Llama3:trigger_response()

            return { prompt = prompt, raw = true, }
          end,
        },
        commit = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.INSERT_OR_REPLACE,
          builder = function()
            local git_diff = vim.fn.system({ 'git', 'diff', '--staged', })

            if git_diff == '' then error('Empty git diff') end

            local prompt = Llama3:system_prompt(
                  "You're a Software Engineer who write clear and succinct commits following the Convetional Commits convention."
                )
                .. Llama3:user_prompt(
                  'Write just a commit message for the following git diff with conventional commit type in lowercase: '
                  .. Llama3.code_prompt(git_diff)
                )
                .. Llama3:trigger_response()

            return { prompt = prompt, raw = true, }
          end,
        },
        test = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.APPEND,
          builder = function(code)
            local lang = vim.fn.input('Language: ')

            if lang == '' then error('No language supplied') end

            local prompt = Llama3:system_prompt(
                  "You're a " .. ' Software Engineer expert in writing well design and documented tests.')
                .. Llama3:user_prompt(
                  'Write tests for the following ' .. lang .. ' code:' .. Llama3.code_prompt(code)
                )
                .. Llama3:trigger_response()

            return { prompt = prompt, raw = true, }
          end,
        },
        refactor = {
          provider = require('model.providers.ollama'),
          params = { model = 'llama3:latest', },
          mode = require('model').mode.INSERT_OR_REPLACE,
          builder = function(code)
            local lang = vim.fn.input('Language: ')

            if lang == '' then error('No language supplied') end

            local prompt = Llama3:prompt_as('system', "You're a Software Engineer expert in " .. lang)
                .. Llama3:user_prompt(
                  'Refactor the following code:' .. Llama3.code_prompt(code)
                )
                .. Llama3:trigger_response()

            return { prompt = prompt, raw = true, }
          end,
        },
      },
    })
  end,
}
