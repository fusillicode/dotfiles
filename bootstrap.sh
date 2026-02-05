#!/usr/bin/env bash

set -euo pipefail

script_dir="${BASH_SOURCE%/*}"
dotfiles_dir="$HOME/data/dev/dotfiles/dotfiles"

# Helper functions for output
info() { echo -e "\033[34m==>\033[0m $1"; }
success() { echo -e "\033[32m==>\033[0m $1"; }
warn() { echo -e "\033[33m==>\033[0m $1"; }

command_exists() { command -v "$1" &>/dev/null; }

# Helper function for idempotent symlinks
safe_link() {
  local src="$1" dest="$2"
  mkdir -p "$(dirname "$dest")"
  ln -sf "$src" "$dest"
}

# Symlink .config directories
info "Symlinking .config directories..."
safe_link "$dotfiles_dir/.config/alacritty" "$HOME/.config/alacritty"
safe_link "$dotfiles_dir/.config/atuin" "$HOME/.config/atuin"
safe_link "$dotfiles_dir/.config/gitui" "$HOME/.config/gitui"
safe_link "$dotfiles_dir/.config/harper-ls" "$HOME/.config/harper-ls"
safe_link "$dotfiles_dir/.config/helix" "$HOME/.config/helix"
safe_link "$dotfiles_dir/.config/mise" "$HOME/.config/mise"
safe_link "$dotfiles_dir/.config/nvim" "$HOME/.config/nvim"
safe_link "$dotfiles_dir/.config/opencode" "$HOME/.config/opencode"
safe_link "$dotfiles_dir/.config/pgcli" "$HOME/.config/pgcli"
safe_link "$dotfiles_dir/.config/starship.toml" "$HOME/.config/starship.toml"

# Symlink dotfiles in home directory
info "Symlinking dotfiles to home directory..."
safe_link "$dotfiles_dir/.hushlogin" "$HOME/.hushlogin"
safe_link "$dotfiles_dir/.gitignore" "$HOME/.gitignore"
safe_link "$dotfiles_dir/.gitignore_global" "$HOME/.gitignore_global"
safe_link "$dotfiles_dir/.myclirc" "$HOME/.myclirc"
safe_link "$dotfiles_dir/.psqlrc" "$HOME/.psqlrc"
safe_link "$dotfiles_dir/.sqruff" "$HOME/.sqruff"
safe_link "$dotfiles_dir/.wezterm.lua" "$HOME/.wezterm.lua"
safe_link "$dotfiles_dir/.zsh_aliases" "$HOME/.zsh_aliases"
safe_link "$dotfiles_dir/.zshrc" "$HOME/.zshrc"
safe_link "$dotfiles_dir/.zsh-fzf-custom-history" "$HOME/.zsh-fzf-custom-history"

# Copy gitconfig (not symlinked to allow local modifications)
cp "$dotfiles_dir/.gitconfig" "$HOME/.gitconfig"

# Homebrew
info "Installing Homebrew..."
if ! command_exists brew; then
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install.sh)"
else
  warn "Homebrew already installed, skipping..."
fi

brew analytics off
brew update
brew doctor --verbose

# Homebrew apps
info "Installing Homebrew packages..."
brew install \
  awscli \
  gh \
  git \
  jq \
  kube-ps1 \
  kubectx \
  libpq \
  librdkafka \
  opencode \
  stern \
  txn2/tap/kubefwd \
  zsh

brew link libpq --force

# Homebrew casks
info "Installing Homebrew casks..."
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
  whatsapp

# Requirements to build nvim
info "Installing nvim build requirements..."
brew install ninja cmake gettext curl

# Xcode tools
info "Installing Xcode command line tools..."
xcode-select --install || warn "Xcode tools may already be installed"

# rustup
info "Installing rustup..."
if ! command_exists rustup; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
else
  warn "rustup already installed, skipping..."
fi

# Source cargo environment
if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

# Cargo bins
info "Installing cargo binaries..."
/bin/bash "$script_dir"/bin/update_cargo_bins.sh

# mise (installed via cargo, now available)
info "Updating mise..."
if command_exists mise; then
  mise self-update
  mise upgrade
else
  warn "mise not found, skipping mise update..."
fi

# Setup ~/.local/bin & ~/.dev_tools
info "Running evoke setup..."
cd yog && cargo run --bin evoke && cd -

# Update & cleanup brew
info "Cleaning up Homebrew..."
/bin/bash "$script_dir"/bin/update_brew.sh

success "Bootstrap complete!"
