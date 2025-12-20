#!/usr/bin/env bash

# Finder
defaults write com.apple.finder DisableAllAnimations -bool true;
defaults write com.apple.finder AppleShowAllFiles -bool true;
defaults write com.apple.finder FXPreferredViewStyle -string "Nlsv";
defaults write NSGlobalDomain AppleShowAllExtensions -bool true;
defaults write com.apple.finder FXEnableExtensionChangeWarning -bool false;
defaults write com.apple.finder _FXSortFoldersFirst -bool true;
defaults write com.apple.finder _FXSortFoldersFirstOnDesktop -bool true;
sudo find / -name ".DS_Store" -delete;

# Dock
defaults write com.apple.dock autohide-time-modifier -float 0;
defaults write com.apple.dock autohide-delay -float 0;
defaults write com.apple.dock static-only -bool true;
defaults write com.apple.dock show-recents -bool false;

# Disable other animations
defaults write NSGlobalDomain NSAutomaticWindowAnimationsEnabled -bool false;
defaults write NSGlobalDomain NSWindowResizeTime -float 0.001;
defaults write -g QLPanelAnimationDuration -float 0;
defaults write com.apple.dock expose-animation-duration -float 0;

# Apply changes by restarting the Dock and Finder
killall Dock; killall Finder
