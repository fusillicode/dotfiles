#!/usr/bin/env bash

# If the Effective User ID (EUID) is not 0 (root), restart the script with sudo.
if [ "$EUID" -ne 0 ]; then
  echo "Need admin privileges, please enter your password:"
  exec sudo -p "" "$0" "$@"
fi

# Update existing sudo time stamp until the script has finished.
while true; do sudo -n true; sleep 60; kill -0 "$$" || exit; done 2>/dev/null &

# Disable general animations.
defaults write -g QLPanelAnimationDuration -float 0;
defaults write NSGlobalDomain NSAutomaticWindowAnimationsEnabled -bool false;
defaults write NSGlobalDomain NSWindowResizeTime -float 0.001;

# Menubar.
defaults -currentHost write com.apple.controlcenter Bluetooth -int 18;
defaults -currentHost write com.apple.controlcenter Sound -int 18;
defaults -currentHost write com.apple.controlcenter Battery -int 18;
defaults -currentHost write com.apple.controlcenter BatteryShowPercentage -bool true;
defaults write com.apple.menuextra.clock ShowDate -int 1;

# Sounds.
defaults write com.apple.systemsound "com.apple.sound.uiaudio.enabled" -int 0;
defaults write com.apple.systemsound "com.apple.sound.beep.volume" -float 0.0
defaults write NSGlobalDomain com.apple.sound.beep.feedback -int 0
nvram StartupMute=%01
killall ControlCenter; killall SystemUIServer;

# Finder.
defaults write NSGlobalDomain AppleShowAllExtensions -bool true;
defaults write com.apple.finder AppleShowAllFiles -bool true;
defaults write com.apple.finder DisableAllAnimations -bool true;
defaults write com.apple.finder FXEnableExtensionChangeWarning -bool false;
defaults write com.apple.finder FXPreferredViewStyle -string "Nlsv";
defaults write com.apple.finder _FXSortFoldersFirst -bool true;
defaults write com.apple.finder _FXSortFoldersFirstOnDesktop -bool true;
/usr/libexec/PlistBuddy \
  -c "Set :DesktopViewSettings:IconViewSettings:arrangeBy grid" \
  ~/Library/Preferences/com.apple.finder.plist;
find / -name ".DS_Store" -delete;
killall Finder;

# Dock.
defaults write com.apple.dock autohide-delay -float 0;
defaults write com.apple.dock autohide-time-modifier -float 0;
defaults write com.apple.dock expose-animation-duration -float 0;
defaults write com.apple.dock mineffect -string "scale";
defaults write com.apple.dock minimize-to-application -bool true;
defaults write com.apple.dock show-recents -bool false;
defaults write com.apple.dock static-only -bool true;
defaults write com.apple.dock wvous-bl-corner -int 13;
killall Dock;
