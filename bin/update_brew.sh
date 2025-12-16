#!/usr/bin/env bash

set -euo pipefail

brew update
brew upgrade

brew cleanup -s
rm -rf ~/Library/Caches/Homebrew/*
brew doctor
