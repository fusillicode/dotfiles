return {
  {
    'CopilotC-Nvim/CopilotChat.nvim',
    dependencies = {
      { 'nvim-lua/plenary.nvim', branch = 'master', },
    },
    build = 'make tiktoken',
    config = function()
      require('CopilotChat').setup({
        providers = {
          gemini = {
            prepare_input = require('CopilotChat.config.providers').copilot.prepare_input,
            prepare_output = require('CopilotChat.config.providers').copilot.prepare_output,

            get_headers = function()
              local api_key = assert(os.getenv('GEMINI_API_KEY'), 'GEMINI_API_KEY env not set')
              return {
                Authorization = 'Bearer ' .. api_key,
                ['Content-Type'] = 'application/json',
              }
            end,

            get_models = function(headers)
              local response, err = require('CopilotChat.utils').curl_get(
                'https://generativelanguage.googleapis.com/v1beta/openai/models', {
                  headers = headers,
                  json_response = true,
                })

              if err then
                error(err)
              end

              return vim.tbl_map(function(model)
                local id = model.id:gsub('^models/', '')
                return {
                  id = id,
                  name = id,
                }
              end, response.body.data)
            end,

            embed = function(inputs, headers)
              local response, err = require('CopilotChat.utils').curl_post(
                'https://generativelanguage.googleapis.com/v1beta/openai/embeddings', {
                  headers = headers,
                  json_request = true,
                  json_response = true,
                  body = {
                    input = inputs,
                    model = 'text-embedding-004',
                  },
                })

              if err then
                error(err)
              end

              return response.body.data
            end,

            get_url = function()
              return 'https://generativelanguage.googleapis.com/v1beta/openai/chat/completions'
            end,
          },
        },
      })
    end,
  },
}
