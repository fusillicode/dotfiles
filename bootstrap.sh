#!/usr/bin/env sh

# Install Xcode tools
xcode-select --install

# Setup Homebrew
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/uninstall.sh)"
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install.sh)"
brew update
brew doctor --verbose

# Install Homebrew apps
brew install ansible
brew install awscli
brew install cpulimit
brew install gh
brew install git
brew install hadolint
brew install hashicorp/tap/terraform-ls
brew install jq
brew install kube-ps1
brew install kubectx
brew install kustomize
brew install lftp
brew install libpq && brew link libpq --force
brew install librdkafka # It also installs `lz4`, `lzlib` & `zstd`
brew install marksman
brew install mycli # For Python `mysqlclient`
brew install stern
brew install txn2/tap/kubefwd
brew install vegeta
brew install watch
brew install yq
brew install zsh

# Install Homebrew casks
brew install android-file-transfer
brew install appcleaner
brew install bitbar
brew install chromedriver
brew install discord
brew install firefox
brew install google-chrome
brew install helix
brew install homebrew/cask/docker
brew install iterm2
brew install league-of-legends
brew install rectangle
brew install slack
brew install telegram
brew install the-unarchiver
brew install transmission --cask
brew install tunnelblick
brew install vlc
brew install whatsapp

# Install Oh My Zsh
sh -c "$(curl -fsSL https://raw.githubusercontent.com/robbyrussell/oh-my-zsh/master/tools/install.sh)"

# Install rustup ❤️
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Rust bins ❤️
cargo install atuin cargo-make git-delta ripgrep rtx-cli sd sqlx-cli taplo-cli

# Configure atuin
echo 'eval "$(atuin init zsh)"' >> ~/.zshrc
# Configure rtx
echo 'eval "$(rtx activate zsh)"' >> ~/.zshrc

# Install rtx plugins
brew install autoconf wxwidgets
export KERL_CONFIGURE_OPTIONS="--without-javac --with-ssl=$(brew --prefix openssl@1.1)"
rtx install elixir latest
rtx install elm latest
rtx install erlang latest
rtx install node latest
rtx install poetry latest
rtx install python latest

# Install Elixir libs
mix local.hex
mix local.rebar
mix archive.install hex phx_new 1.5.8

# Install Node stuff...
npm install -g yarn

# Setup Helix LSP
pip install ruff python-lsp-server python-lsp-ruff
npm install -g @elm-tooling/elm-language-server bash-language-server yaml-language-server vscode-languageservers-extracted

# Upgrade and cleanup brew stuff...
brew update && brew upgrade && brew cleanup -s && rm -rf ~/Library/Caches/Homebrew/*
brew doctor
