#!/usr/bin/env bash

set -euo pipefail

script_dir="${BASH_SOURCE%/*}"

# shellcheck source=log.sh
source "$script_dir/log.sh"

# Preserve original user when running with sudo
REAL_USER="${SUDO_USER:-$USER}"
REAL_HOME=$(eval echo "~$REAL_USER")

# If the Effective User ID (EUID) is not 0 (root), restart the script with sudo.
if [ "$EUID" -ne 0 ]; then
  info "Need admin privileges, please enter your password:"
  exec sudo -p "" "$0" "$@"
fi

info "Configuring macOS settings for user: $REAL_USER"

# Update existing sudo time stamp until the script has finished.
while true; do sudo -n true; sleep 60; kill -0 "$$" || exit; done 2>/dev/null &

# Animations.
info "Configuring animations..."
defaults write -g QLPanelAnimationDuration -float 0
defaults write NSGlobalDomain NSAutomaticWindowAnimationsEnabled -bool false
defaults write NSGlobalDomain NSWindowResizeTime -float 0.001
ok "Animations configured"

# Keyboard.
info "Configuring keyboard..."
defaults write -g ApplePressAndHoldEnabled -bool false
defaults write -g InitialKeyRepeat -int 10
defaults write -g KeyRepeat -int 0
ok "Keyboard configured"

# Menubar.
info "Configuring menubar..."
defaults -currentHost write com.apple.controlcenter Bluetooth -int 18
defaults -currentHost write com.apple.controlcenter Sound -int 18
defaults -currentHost write com.apple.controlcenter Battery -int 18
defaults -currentHost write com.apple.controlcenter BatteryShowPercentage -bool true
defaults write com.apple.menuextra.clock ShowDate -int 1
ok "Menubar configured"

# Sounds.
info "Configuring sounds..."
defaults write com.apple.systemsound "com.apple.sound.uiaudio.enabled" -int 0
defaults write com.apple.systemsound "com.apple.sound.beep.volume" -float 0.0
defaults write NSGlobalDomain com.apple.sound.beep.feedback -int 0
nvram StartupMute=%01
killall ControlCenter 2>/dev/null || true
killall SystemUIServer 2>/dev/null || true
ok "Sounds configured"

# Finder.
info "Configuring Finder..."
defaults write NSGlobalDomain AppleShowAllExtensions -bool true
defaults write com.apple.finder AppleShowAllFiles -bool true
defaults write com.apple.finder DisableAllAnimations -bool true
defaults write com.apple.finder FXEnableExtensionChangeWarning -bool false
defaults write com.apple.finder FXPreferredViewStyle -string "Nlsv"
defaults write com.apple.finder WarnOnEmptyTrash -bool false
defaults write com.apple.finder _FXSortFoldersFirst -bool true
defaults write com.apple.finder _FXSortFoldersFirstOnDesktop -bool true
# List view: sort by date modified, ascending (oldest first)
/usr/libexec/PlistBuddy \
  -c "Set :StandardViewSettings:ExtendedListViewSettingsV2:sortColumn dateModified" \
  -c "Set :StandardViewSettings:ExtendedListViewSettingsV2:sortDirection 0" \
  -c "Set :StandardViewSettings:ListViewSettings:sortColumn dateModified" \
  -c "Set :StandardViewSettings:ListViewSettings:sortDirection 1" \
  "$REAL_HOME/Library/Preferences/com.apple.finder.plist" 2>/dev/null || true
# Desktop: arrange by date modified, ascending (oldest first)
/usr/libexec/PlistBuddy \
  -c "Set :DesktopViewSettings:IconViewSettings:arrangeBy dateModified" \
  -c "Set :DesktopViewSettings:IconViewSettings:sortDirection 1" \
  "$REAL_HOME/Library/Preferences/com.apple.finder.plist" 2>/dev/null || true
# Delete .DS_Store files (fd is faster and auto-ignores .git/node_modules)
info "Deleting .DS_Store files..."
fd -H -t f '\.DS_Store$' "$REAL_HOME" -x rm -v 2>/dev/null || true
killall Finder 2>/dev/null || true
ok "Finder configured"

# Dock.
info "Configuring Dock..."
defaults write com.apple.dock autohide-delay -float 0
defaults write com.apple.dock autohide-time-modifier -float 0
defaults write com.apple.dock expose-animation-duration -float 0
defaults write com.apple.dock mineffect -string "scale"
defaults write com.apple.dock minimize-to-application -bool true
defaults write com.apple.dock show-recents -bool false
defaults write com.apple.dock static-only -bool true
defaults write com.apple.dock wvous-bl-corner -int 13
defaults write com.apple.dock wvous-br-corner -int 0
defaults write com.apple.dock wvous-br-modifier -int 0
killall Dock 2>/dev/null || true
ok "Dock configured"

ok "macOS configuration complete!"
