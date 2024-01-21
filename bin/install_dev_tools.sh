#!/bin/bash

set -euo pipefail

dev_tools_dir="$HOME"/.dev-tools
bin_dir="$HOME"/.local/bin
mkdir -p "$dev_tools_dir" "$bin_dir"

get_latest_release() {
  gh api repos/"$1"/releases/latest --jq '.tag_name'
}

# shellcheck disable=SC1091
install_python_bin() {
  local tool_dir="$1"/"$2"
  mkdir -p "$tool_dir" && \
    cd "$_" && \
    python3 -m venv .venv && \
    source .venv/bin/activate && \
    pip install --upgrade pip && \
    pip install "$2" && \
    cd - && \
    ln -sf "$tool_dir"/.venv/bin/"$2" "$3"
}

if ! gh auth status; then
  gh auth login
fi

curl -SL https://github.com/rust-lang/rust-analyzer/releases/download/nightly/rust-analyzer-aarch64-apple-darwin.gz | \
  zcat > "$bin_dir"/rust-analyzer

curl -SL https://github.com/tamasfe/taplo/releases/latest/download/taplo-full-darwin-aarch64.gz | \
  zcat > "$bin_dir"/taplo

latest_release=$(get_latest_release "hashicorp/terraform-ls" | cut -c2-)
curl -SL https://releases.hashicorp.com/terraform-ls/"$latest_release"/terraform-ls_"$latest_release"_darwin_arm64.zip | \
  tar -xz -C "$bin_dir"

repo="tekumara/typos-vscode"
latest_release=$(get_latest_release $repo)
curl -SL https://github.com/"$repo"/releases/download/"$latest_release"/typos-lsp-"$latest_release"-aarch64-apple-darwin.tar.gz | \
  tar -xz -C "$bin_dir"

repo="errata-ai/vale"
latest_release=$(get_latest_release $repo)
curl -SL https://github.com/"$repo"/releases/download/"$latest_release"/vale_"$(echo "$latest_release" | cut -c2-)"_macOS_arm64.tar.gz | \
  tar -xz -C "$bin_dir"

curl -SL https://github.com/hadolint/hadolint/releases/latest/download/hadolint-Darwin-x86_64 --output "$bin_dir"/hadolint
curl -SL https://github.com/mrjosh/helm-ls/releases/latest/download/helm_ls_darwin_amd64 --output "$bin_dir"/helm_ls
curl -SL https://github.com/artempyanykh/marksman/releases/latest/download/marksman-macos --output "$bin_dir"/marksman

tool="shellcheck"
repo="koalaman/$tool"
latest_release=$(get_latest_release $repo)
unarchived_dir="$dev_tools_dir"/"$tool"-"$latest_release"
curl -SL https://github.com/"$repo"/releases/download/"$latest_release"/"$tool"-"$latest_release".darwin.x86_64.tar.xz | \
  tar -xz -C "$dev_tools_dir" && \
  mv "$unarchived_dir"/"$tool" "$bin_dir" && \
  rm -rf "$unarchived_dir"

tool="elixir-ls"
repo="elixir-lsp/$tool"
dev_tools_repo_dir="$dev_tools_dir"/"$tool"
latest_release=$(get_latest_release $repo)
mkdir -p "$dev_tools_repo_dir" && \
  curl -SL https://github.com/"$repo"/releases/download/"$latest_release"/"$tool"-"$latest_release".zip | \
  tar -xz -C "$dev_tools_repo_dir" && \
  chmod +x "$dev_tools_repo_dir"/* && \
  ln -sf "$dev_tools_repo_dir"/language_server.sh "$bin_dir"/elixir-ls
  ln -sf "$dev_tools_repo_dir"/debug_adapter.sh "$bin_dir"/elixir-ls-debugger

# No `bin` link as it requires some local stuff so, leave the garbage in `dev-tools` and configure the LSP to point to
# the `bin` there.
# TODO: commented out because it's not working with Elixir 1.16 🤷
# repo="lexical"
# dev_tools_repo_dir="$dev_tools_dir"/"$repo"
# rm -rf "$dev_tools_repo_dir" && \
#   mkdir -p "$dev_tools_repo_dir" && \
#   cd "$_" && \
#   git clone git@github.com:"$repo"-lsp/"$repo".git --depth=1 --single-branch --branch=main . && \
#   mix deps.get > /dev/null && \
#   mix package > /dev/null

# No `bin` link as it requires some local stuff so, leave the garbage in `dev-tools` and configure the LSP to point to
# the `bin` there.
tool="lua-language-server"
repo="LuaLS/$tool"
latest_release=$(get_latest_release $repo)
dev_tool_dir="$dev_tools_dir"/"$tool"
 mkdir -p "$dev_tool_dir" && \
  curl -SL https://github.com/"$repo"/releases/download/"$latest_release"/"$tool"-"$latest_release"-darwin-arm64.tar.gz | \
  tar -xz -C "$dev_tool_dir"

mkdir -p "$dev_tools_dir"/phpactor && \
  composer require --dev --working-dir "$_" phpactor/phpactor > /dev/null &&
  ln -sf "$dev_tools_dir"/phpactor/vendor/bin/phpactor "$bin_dir"

mkdir -p "$dev_tools_dir"/php-cs-fixer && \
  composer require --dev --working-dir "$_" friendsofphp/php-cs-fixer > /dev/null &&
  ln -sf "$dev_tools_dir"/php-cs-fixer/vendor/bin/php-cs-fixer "$bin_dir"

mkdir -p "$dev_tools_dir"/psalm && \
  composer require --dev --working-dir "$_" vimeo/psalm > /dev/null &&
  ln -sf "$dev_tools_dir"/psalm/vendor/bin/* "$bin_dir"

mkdir -p "$dev_tools_dir"/commitlint && \
  npm install --silent --prefix "$_" @commitlint/{cli,config-conventional} && \
  ln -sf "$dev_tools_dir"/commitlint/node_modules/.bin/commitlint "$bin_dir"

mkdir -p "$dev_tools_dir"/elm-language-server && \
  npm install --silent --prefix "$_" @elm-tooling/elm-language-server && \
  ln -sf "$dev_tools_dir"/elm-language-server/node_modules/.bin/elm-language-server "$bin_dir"

mkdir -p "$dev_tools_dir"/compose-language-service && \
  npm install --silent --prefix "$_" @microsoft/compose-language-service && \
  ln -sf "$dev_tools_dir"/compose-language-service/node_modules/.bin/docker-compose-langserver "$bin_dir"

mkdir -p "$dev_tools_dir"/bash-language-server && \
  npm install --silent --prefix "$_" bash-language-server && \
  ln -sf "$dev_tools_dir"/bash-language-server/node_modules/.bin/bash-language-server "$bin_dir"

mkdir -p "$dev_tools_dir"/dockerfile-language-server-nodejs && \
  npm install --silent --prefix "$_" dockerfile-language-server-nodejs && \
  ln -sf "$dev_tools_dir"/dockerfile-language-server-nodejs/node_modules/.bin/docker-langserver "$bin_dir"

mkdir -p "$dev_tools_dir"/graphql-language-service-cli && \
  npm install --silent --prefix "$_" graphql-language-service-cli && \
  ln -sf "$dev_tools_dir"/graphql-language-service-cli/node_modules/.bin/graphql-lsp "$bin_dir"

mkdir -p "$dev_tools_dir"/prettier && \
  npm install --silent --prefix "$_" prettier && \
  ln -sf "$dev_tools_dir"/prettier/node_modules/.bin/prettier "$bin_dir"

mkdir -p "$dev_tools_dir"/sql-language-server && \
  npm install --silent --prefix "$_" sql-language-server && \
  ln -sf "$dev_tools_dir"/sql-language-server/node_modules/.bin/sql-language-server "$bin_dir"

mkdir -p "$dev_tools_dir"/vscode-langservers-extracted && \
  npm install --silent --prefix "$_" vscode-langservers-extracted && \
  ln -sf "$dev_tools_dir"/vscode-langservers-extracted/node_modules/.bin/* "$bin_dir"

mkdir -p "$dev_tools_dir"/yaml-language-server && \
  npm install --silent --prefix "$_" yaml-language-server && \
  ln -sf "$dev_tools_dir"/yaml-language-server/node_modules/.bin/yaml-language-server "$bin_dir"

mkdir -p "$dev_tools_dir"/typescript-language-server && \
  npm install --silent --prefix "$_" typescript-language-server typescript && \
  ln -sf "$dev_tools_dir"/typescript-language-server/node_modules/.bin/typescript-language-server "$bin_dir"

mkdir -p "$dev_tools_dir"/quicktype && \
  npm install --silent --prefix "$_" quicktype && \
  ln -sf "$dev_tools_dir"/quicktype/node_modules/.bin/quicktype "$bin_dir"

install_python_bin "$dev_tools_dir" ruff-lsp "$bin_dir"

chmod +x "$bin_dir"/*
