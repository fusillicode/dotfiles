#!/usr/bin/env bash

script_dir="${BASH_SOURCE%/*}"
dotfiles_dir="$HOME/data/dev/dotfiles/dotfiles"

# Symlink configs
ln -s "$dotfiles_dir/.config/atuin/" "$HOME/.config/atuin"
ln -s "$dotfiles_dir/.config/gitui/" "$HOME/.config/gitui"
ln -s "$dotfiles_dir/.config/helix/" "$HOME/.config/helix"
ln -s "$dotfiles_dir/.config/mise/" "$HOME/.config/mise"
ln -s "$dotfiles_dir/.config/nvim/" "$HOME/.config/nvim"
ln -s "$dotfiles_dir/.config/pgcli/config" "$HOME/.config/pgcli/config"

cp "$dotfiles_dir/.gitconfig" "$HOME"
ln -s "$dotfiles_dir/.gitignore" "$HOME"
ln -s "$dotfiles_dir/.gitignore_global" "$HOME"
ln -s "$dotfiles_dir/.myclirc" "$HOME"
ln -s "$dotfiles_dir/.psqlrc" "$HOME"
ln -s "$dotfiles_dir/.psqlrc" "$HOME"
ln -s "$dotfiles_dir/.vale.ini" "$HOME"
ln -s "$dotfiles_dir/.wezterm" "$HOME"
ln -s "$dotfiles_dir/.zshenv" "$HOME"
ln -s "$dotfiles_dir/.zshrc" "$HOME"
ln -s "$dotfiles_dir/my-zsh.zsh-theme" "$HOME/.oh-my-zsh/custom/themes"

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

# Requirements for PHP...
# https://github.com/asdf-community/asdf-php/blob/1eaf4de9b86bd0a45aa7ac3698d01d646a9b1037/.github/workflows/workflow.yml#L52
brew install autoconf automake bison freetype gd gettext icu4c krb5 libedit libiconv libjpeg libpng libxml2 libzip pkg-config re2c zlib \
  # Requirements for Erlang...
  # https://github.com/asdf-vm/asdf-erlang?tab=readme-ov-file#osx
  openssl@1 \
  wxwidgets \

# Requirements for Erlang...
# https://github.com/asdf-vm/asdf-erlang?tab=readme-ov-file#use
CC="/usr/bin/clang -I$(brew --prefix openssl)/include"
export CC
LDFLAGS="-L$(brew --prefix openssl)/lib:$LDFLAGS"
export LDFLAGS
KERL_CONFIGURE_OPTIONS="--without-javac --with-ssl=$(brew --prefix openssl)"
export KERL_CONFIGURE_OPTIONS

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
  wezterm@nightly --cask --no-quarantine --greedy-latest \
  whatsapp \

# 🥲 https://wezfurlong.org/wezterm/faq.html#how-do-i-enable-undercurl-curly-underlines
tempfile=$(mktemp) \
  && curl -o "$tempfile" https://raw.githubusercontent.com/wez/wezterm/master/termwiz/data/wezterm.terminfo \
  && tic -x -o ~/.terminfo "$tempfile" \
  && rm "$tempfile" \

# Oh My Zsh
sh -c "$(curl -fsSL https://raw.githubusercontent.com/robbyrussell/oh-my-zsh/master/tools/install.sh)"

# Requirements for nvim
brew install ninja cmake gettext curl

# Setup ~/.local/bin & ~/.dev_tools
cd ./yog && \
  ./install.sh && \
  rm -f "$HOME/.local/bin/update_*" && \
  ln -s "$HOME/data/dev/dotfiles/dotfiles/bin/update_*" "$HOME/.local/bin"

# Update & cleanup brew
/bin/bash "$script_dir/bin/update_brew.sh"
