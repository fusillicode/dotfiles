#!/usr/bin/env bash

script_dir="${BASH_SOURCE%/*}"

# Symlink configs
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.config/atuin/ "$HOME"/.config/atuin
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.config/gitui/ "$HOME"/.config/gitui
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.config/helix/ "$HOME"/.config/helix
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.config/mise/ "$HOME"/.config/mise
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.config/nvim/ "$HOME"/.config/nvim

cp "$HOME"/data/dev/dotfiles/dotfiles/.gitconfig "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.gitignore "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.gitignore_global "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.myclirc "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.psqlrc "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.psqlrc "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.vale.ini "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.wezterm "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.zshenv "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/.zshrc "$HOME"
ln -s "$HOME"/data/dev/dotfiles/dotfiles/my-zsh.zsh-theme "$HOME"/.oh-my-zsh/custom/themes

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

# Setup ~/.dev_tools & ~/.local/bin
cd ./tempura &&
  cargo build --release && \
  cd - && \
  rm "$HOME"/.local/bin/idt && \
  ln -s "$HOME"/data/dev/dotfiles/dotfiles/tempura/target/release/idt "$HOME"/.local/bin && \
  idt ~/.dev-tools ~/.local/bin && \
  ln -s "$HOME"/data/dev/dotfiles/dotfiles/bin/update_* "$HOME"/.local/bin

# Update & cleanup brew
/bin/bash "$script_dir"/bin/update_brew.sh
