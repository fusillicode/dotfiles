#!/usr/bin/env bash

set -euo pipefail

brew update
brew upgrade

# Hardcore stuff 🦾
brew upgrade --cask wezterm@nightly --no-quarantine --greedy-latest

brew cleanup -s
rm -rf ~/Library/Caches/Homebrew/*
brew doctor
