#!/bin/bash

set -euo pipefail

brew update
brew upgrade

# Hardcore stuff 🦾
brew upgrade --cask homebrew/cask-versions/wezterm-nightly --no-quarantine --greedy-latest

brew cleanup -s
rm -rf ~/Library/Caches/Homebrew/*
brew doctor

cd ~/data/dev/neovim || git clone https://github.com/neovim/neovim && \
  git pull origin master && \
  make distclean && \
  make CMAKE_BUILD_TYPE=Release && \
  sudo make install
