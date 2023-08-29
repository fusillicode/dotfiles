#!/bin/bash

script_dir="${BASH_SOURCE%/*}"

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
brew install shellcheck
brew install stern
brew install txn2/tap/kubefwd
brew install vegeta
brew install watch
brew tap homebrew/cask-versions
brew install --cask wez/wezterm/wezterm-nightly --no-quarantine
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

# Install cargo bins ❤️
/bin/bash "${script_dir}/bin/cargobu"
cargo install --force qsv --features all_features

# Configure atuin
# shellcheck disable=SC2016
echo 'eval "$(atuin init zsh)"' >> ~/.zshrc
# Configure rtx
# shellcheck disable=SC2016
echo 'eval "$(rtx activate zsh)"' >> ~/.zshrc

# Install rtx plugins
brew install autoconf wxwidgets
KERL_CONFIGURE_OPTIONS="--without-javac --with-ssl=$(brew --prefix openssl@1.1)"
export KERL_CONFIGURE_OPTIONS
rtx use -g elixir@latest
rtx use -g elm@latest
rtx use -g erlang@latest
rtx use -g node@latest
rtx use -g poetry@latest
rtx use -g python@latest

# Install rtx related tools
/bin/bash "${script_dir}/bin/rtxtu"

# Install Elixir libs
mix local.hex
mix local.rebar
mix archive.install hex phx_new 1.5.8

# Update & cleanup brew
/bin/bash "${script_dir}/bin/brewu"
