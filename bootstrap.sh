#!/usr/bin/env bash

script_dir="${BASH_SOURCE%/*}"

# Xcode tools
xcode-select --install

# rustup ❤️
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Cargo bins ❤️
/bin/bash "${script_dir}/bin/update_cargo_bins.sh"

# Homebrew
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install.sh)"
brew analytics off
brew update
brew doctor --verbose

# Homebrew apps
brew install \
  awscli \
  gh \
  git \
  jq \
  kube-ps1 \
  kubectx \
  kustomize \
  lftp \
  libpq \
  librdkafka \
  mycli \
  stern \
  txn2/tap/kubefwd \
  vegeta \
  zsh \

brew link libpq --force

# Homebrew casks
brew tap homebrew/cask-versions
brew install \
  alt-tab --cask \
  appcleaner \
  bitbar \
  chromedriver \
  discord \
  firefox \
  google-chrome \
  keepingyouawake \
  orbstack \
  rectangle \
  slack \
  telegram \
  the-unarchiver \
  transmission --cask \
  vlc \
  wez/wezterm/wezterm-nightly --cask --no-quarantine \
  whatsapp \

# 🥲 https://wezfurlong.org/wezterm/faq.html#how-do-i-enable-undercurl-curly-underlines
tempfile=$(mktemp) \
  && curl -o "$tempfile" https://raw.githubusercontent.com/wez/wezterm/master/termwiz/data/wezterm.terminfo \
  && tic -x -o ~/.terminfo "$tempfile" \
  && rm "$tempfile" \

# Oh My Zsh
sh -c "$(curl -fsSL https://raw.githubusercontent.com/robbyrussell/oh-my-zsh/master/tools/install.sh)"

# Requirements for Erlang 🥲
brew install autoconf wxwidgets
CC="/usr/bin/clang -I$(brew --prefix openssl)/include"
export CC
LDFLAGS="-L$(brew --prefix openssl)/lib:$LDFLAGS"
export LDFLAGS
KERL_CONFIGURE_OPTIONS="--without-javac --with-ssl=$(brew --prefix openssl)"
export KERL_CONFIGURE_OPTIONS

# Setup ~/.dev_tools & ~/.local/bin
./bin/tempura/target/release/tempura install-dev-tools ~/.dev-tools ~/.local/bin

# Update & cleanup brew
/bin/bash "${script_dir}/bin/update_brew.sh"
