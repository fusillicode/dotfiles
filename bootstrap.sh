#!/usr/bin/env sh

# Install Xcode tools
xcode-select --install

# Setup Homebrew
ruby -e "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/uninstall)"
ruby -e "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install)"
brew update
brew doctor
brew prune
brew tap homebrew/dupes
brew tap homebrew/versions
brew tap homebrew/homebrew-php
brew tap caskroom/cask
brew tap caskroom/versions

# Install Homebrew apps
brew install awscli
brew install boot2docker
brew install docker
brew install elixir
brew install git
brew install gpg
brew install heroku-toolbelt
brew install imagemagick
brew install mackup
brew install mysql
brew install php55
brew unlink php55
brew install php56
brew install adminer
brew install composer
brew install postgres
brew install redis
brew install youtube-dl
brew install zsh

# Install Homebrew-Cask
brew install caskroom/cask/brew-cask
brew update && brew upgrade brew-cask && brew cleanup && brew cask cleanup

# Install Homebrew-Cask apps
brew cask install adium
brew cask install appcleaner
brew cask install dropbox
brew cask install google-chrome
brew cask install filezilla
brew cask install firefox
brew cask install istat-menus
brew cask install libreoffice
brew cask install mpv
brew cask install screenhero
brew cask install slack
brew cask install skype
brew cask install sublime-text3
brew cask install transmission
brew cask install vagrant
brew cask install virtualbox

# Upgrade everything and remove outdated versions from the cellar
brew update && brew upgrade && brew cleanup -n && brew cask cleanup

# Install PHP Switcher Script
mkdir -p ~/bin/
curl -L https://raw.githubusercontent.com/conradkleinespel/sphp-osx/master/sphp > ~/bin/sphp
chmod +x ~/bin/sphp
# This should already be handled by other stuff
# echo "export PATH=/usr/local/bin:/usr/local/sbin:$PATH:/Users/`whoami`/bin" >> ~/.profile

# Install NVM
curl -o- https://raw.githubusercontent.com/creationix/nvm/v0.26.1/install.sh | bash

# TODO Install Node stable and make it default
# . ~/.nvm/nvm.sh
# nvm install stable
# nvm alias stable

# Install RVM and Ruby stable
gpg --keyserver hkp://keys.gnupg.net --recv-keys 409B6B1796C275462A1703113804BB82D39DC0E3
curl -sSL https://get.rvm.io | bash -s stable --autolibs=enable --auto-dotfiles --ruby
echo "gem: --no-document" >> ~/.gemrc

# TODO Install gems
# . ~/.rvm/scripts/rvm
# gem install bundler
# gem install wordmove
# gem install compass

# Install Oh My Zsh
curl -L https://github.com/robbyrussell/oh-my-zsh/raw/master/tools/install.sh | sh

# Set up MySQL
cd ~
mysql.server stop
unset TMPDIR
mysql_install_db --verbose --user=`whoami` --basedir="$(brew --prefix mysql)" --datadir=/usr/local/var/mysql --tmpdir=/tmp
mysql.server start
mysql_secure_installation

# Set up git
git config --global push.default current
git config --global core.filemode false
