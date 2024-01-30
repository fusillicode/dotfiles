#!/bin/bash

set -euo pipefail

brew update
brew upgrade

# Hardcore stuff 🦾
brew upgrade --cask homebrew/cask-versions/wezterm-nightly --no-quarantine --greedy-latest
brew upgrade neovim --fetch-HEAD

brew cleanup -s
rm -rf ~/Library/Caches/Homebrew/*
brew doctor
