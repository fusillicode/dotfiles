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
brew install jq
brew install kube-ps1
brew install kubectx
brew install kustomize
brew install lftp
brew install libpq && brew link libpq --force
brew install librdkafka # It also installs `lz4`, `lzlib` & `zstd`
brew install mycli # For Python `mysqlclient`
brew install stern
brew install txn2/tap/kubefwd
brew install vegeta
brew install watch
brew install zsh

brew tap homebrew/cask-versions

# Install Homebrew casks
brew install alt-tab --cask
brew install appcleaner
brew install bitbar
brew install chromedriver
brew install discord
brew install firefox
brew install google-chrome
brew install keepingyouawake
brew install league-of-legends
brew install orbstack
brew install rectangle
brew install slack
brew install telegram
brew install the-unarchiver
brew install transmission --cask
brew install vlc
brew install wez/wezterm/wezterm-nightly --cask --no-quarantine
# 🥲 https://wezfurlong.org/wezterm/faq.html#how-do-i-enable-undercurl-curly-underlines
tempfile=$(mktemp) \
  && curl -o "$tempfile" https://raw.githubusercontent.com/wez/wezterm/master/termwiz/data/wezterm.terminfo \
  && tic -x -o ~/.terminfo "$tempfile" \
  && rm "$tempfile"
brew install whatsapp

# Install Oh My Zsh
sh -c "$(curl -fsSL https://raw.githubusercontent.com/robbyrussell/oh-my-zsh/master/tools/install.sh)"

# Install rustup ❤️
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install cargo bins ❤️
/bin/bash "${script_dir}/bin/update_cargo_bins.sh"

# Configure atuin
# shellcheck disable=SC2016
echo 'eval "$(atuin init zsh)"' >> ~/.zshrc
# Configure mise
# shellcheck disable=SC2016
echo 'eval "$(mise activate zsh)"' >> ~/.zshrc

# Requirements for installning Erlang... 🥲
brew install autoconf wxwidgets
CC="/usr/bin/clang -I$(brew --prefix openssl)/include"
export CC
LDFLAGS="-L$(brew --prefix openssl)/lib:$LDFLAGS"
export LDFLAGS
KERL_CONFIGURE_OPTIONS="--without-javac --with-ssl=$(brew --prefix openssl)"
export KERL_CONFIGURE_OPTIONS

# Install ~/.dev_tools
./bin/tempura/target/release/tempura install-dev-tools ~/.dev-tools ~/.local/bin

# Install Elixir libs
mix local.hex
mix local.rebar

# Update & cleanup brew
/bin/bash "${script_dir}/bin/update_brew.sh"
