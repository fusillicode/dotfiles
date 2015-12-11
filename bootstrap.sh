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
brew tap beeftornado/rmtree
brew tap caskroom/cask
brew tap caskroom/versions

# Install Homebrew apps
brew install awscli
brew install beeftornado/rmtree/brew-rmtree
brew install cputhrottle
brew install elixir
brew install git
brew install gpg
brew install heroku-toolbelt
brew install imagemagick
brew install jq
brew install mackup
brew install mcrypt
brew install mysql
brew install neovim/neovim/neovim
brew install php56
brew install php56-mcrypt
brew install phpunit
brew install composer
brew install postgres
brew install qt
brew install redis
brew install wp-cli
brew install youtube-dl
brew install zsh

# Install Homebrew-Cask
brew install caskroom/cask/brew-cask
brew update && brew upgrade brew-cask && brew cleanup && brew cask cleanup

# Install Homebrew-Cask apps
brew cask install adium
brew cask install appcleaner
brew cask install virtualbox
brew cask install dockertoolbox
brew cask install dropbox
brew cask install google-chrome
brew cask install filezilla
brew cask install firefox
brew cask install istat-menus
brew cask install libreoffice
brew cask install mpv
brew cask install poedit
brew cask install rcdefaultapp
brew cask install screenhero
brew cask install slack
brew cask install skype
brew cask install sublime-text3
brew cask install transmission
brew cask install vagrant

# Upgrade everything and remove outdated versions from the cellar
brew update && brew upgrade && brew cleanup -n && brew cask cleanup

# Install PHP Switcher Script, Docker cleanup and ECS deploy utilities
mkdir -p ~/bin/
curl -L https://raw.githubusercontent.com/fusillicode/dotfiles/master/docker-cleanup.sh > ~/bin/docker-cleanup
curl -L https://raw.githubusercontent.com/silinternational/ecs-deploy/master/ecs-deploy > ~/bin/ecs-deploy
curl -L https://raw.githubusercontent.com/conradkleinespel/sphp-osx/master/sphp > ~/bin/sphp
chmod +x ~/bin/*
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
# gem install compass
# gem install rake
# gem install v8
# gem install wordmove

# Install Oh My Zsh
curl -L https://github.com/robbyrussell/oh-my-zsh/raw/master/tools/install.sh | sh

# Symlink Firefox to global Applications directory to fix Selenium driver
ln -s ~/Applications/Firefox.app /Applications/

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

# Set up Vim temp files directories
mkdir -p ~/.vim/.backup ~/.vim/.swp ~/.vim/.undo

# Set up symlinks for NeoVim
ln -s ~/.vim ~/.config/nvim
ln -s ~/.vimrc ~/.config/nvim/init.vim
