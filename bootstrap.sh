#!/usr/bin/env bash

script_dir="${BASH_SOURCE%/*}"
dotfiles_dir="$HOME/data/dev/dotfiles/dotfiles"

# Symlink configs
ln -s "$dotfiles_dir/.config/alacritty/" "$HOME/.config"
# ln -s "$dotfiles_dir/.config/atuin/" "$HOME/.config"
# ln -s "$dotfiles_dir/.config/gitui/" "$HOME/.config"
# ln -s "$dotfiles_dir/.config/helix/" "$HOME/.config"
ln -s "$dotfiles_dir/.config/mise/" "$HOME/.config"
ln -s "$dotfiles_dir/.config/nvim/" "$HOME/.config"
ln -s "$dotfiles_dir/.config/pgcli/config" "$HOME/.config/pgcli/config"
# ln -s "$dotfiles_dir/.config/opencode" "$HOME/.config/opencode"
ln -s "$dotfiles_dir/.config/harper-ls" "$HOME/.config"
ln -s "$dotfiles_dir/.config/starship.toml" "$HOME/.config/starship.toml"

cp "$dotfiles_dir/.gitconfig" "$HOME"
ln -s "$dotfiles_dir/.gitignore" "$HOME"
ln -s "$dotfiles_dir/.gitignore_global" "$HOME"
# ln -s "$dotfiles_dir/.myclirc" "$HOME"
ln -s "$dotfiles_dir/.psqlrc" "$HOME"
# ln -s "$dotfiles_dir/.wezterm" "$HOME"
ln -s "$dotfiles_dir/.zshenv" "$HOME"
ln -s "$dotfiles_dir/.zshrc" "$HOME"

# Xcode tools
xcode-select --install

# rustup ❤️
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Cargo bins ❤️
/bin/bash "$script_dir"/bin/update_cargo_bins.sh

# Homebrew
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install.sh)"
brew analytics off
brew update
brew doctor --verbose

mise self-update
mise upgrade

# Homebrew apps
brew install \
  awscli \
  gh \
  git \
  jq \
  kube-ps1 \
  kubectx \
  libpq \
  librdkafka \
  stern \
  txn2/tap/kubefwd \
  zsh \

brew link libpq --force

# Homebrew casks
brew install \
  alt-tab --cask \
  alacritty --cask \
  appcleaner \
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
  wezterm@nightly --cask --no-quarantine \
  whatsapp \

# 🥲 https://wezfurlong.org/wezterm/faq.html#how-do-i-enable-undercurl-curly-underlines
tempfile=$(mktemp) \
  && curl -o "$tempfile" https://raw.githubusercontent.com/wez/wezterm/master/termwiz/data/wezterm.terminfo \
  && tic -x -o ~/.terminfo "$tempfile" \
  && rm "$tempfile" \

# Oh My Zsh
sh -c "$(curl -fsSL https://raw.githubusercontent.com/robbyrussell/oh-my-zsh/master/tools/install.sh)"

# Requirements to build nvim
brew install ninja cmake gettext curl

# Setup ~/.local/bin & ~/.dev_tools
cd ./yog && \
  ./install.sh && \
  rm -f "$HOME/.local/bin/update_*" && \
  ln -s "$HOME/data/dev/dotfiles/dotfiles/bin/update_*" "$HOME/.local/bin"

# Update & cleanup brew
/bin/bash "$script_dir"/bin/update_brew.sh
