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
brew install cabal-install
brew install cputhrottle
brew install docker-clean
brew install drush
brew install elixir
brew install exercism
brew install ghc
brew install git
brew install gpg
brew install heroku-toolbelt
brew install hybridgroup/tools/gort
brew install imagemagick
brew install jq
brew install leiningen
brew install mackup
brew install mcrypt
brew install mysql
brew install mpv
brew install php56
brew install php56-mcrypt
brew install phpunit
brew install composer
brew install postgres
brew install qt
brew install redis
brew install rust
brew install sbt
brew install scala
brew install siege
brew install wp-cli
brew install youtube-dl
brew install zsh

# Install Homebrew-Cask
brew install caskroom/cask/brew-cask
brew update && brew upgrade brew-cask && brew cleanup && brew cask cleanup

# Install Homebrew-Cask apps
brew cask install adium
brew cask install appcleaner
brew cask install arduino
brew cask install atom
brew cask install dropbox
brew cask install elm-platform
brew cask install filezilla
brew cask install firefox
brew cask install google-chrome
brew cask install istat-menus
brew cask install java
brew cask install libreoffice
brew cask install poedit
brew cask install rcdefaultapp
brew cask install slack
brew cask install skype
brew cask install spectacle
brew cask install the-unarchiver
brew cask install transmission

# Upgrade everything and remove outdated versions from the cellar
brew update && brew upgrade && brew cleanup -n && brew cask cleanup

# Install PHP Switcher Script, Docker cleanup, ECS deploy and Rebar
mkdir -p ~/bin/
# curl -L https://raw.githubusercontent.com/fusillicode/dotfiles/master/docker-cleanup.sh > ~/bin/docker-cleanup
curl -L https://raw.githubusercontent.com/silinternational/ecs-deploy/master/ecs-deploy > ~/bin/ecs-deploy
curl -L https://raw.githubusercontent.com/conradkleinespel/sphp-osx/master/sphp > ~/bin/sphp
curl -L https://s3.amazonaws.com/rebar3/rebar3 > ~/bin/rebar3
chmod +x ~/bin/*
# This should already be handled by other stuff
# echo "export PATH=/usr/local/bin:/usr/local/sbin:$PATH:/Users/`whoami`/bin" >> ~/.profile

# Install NVM
curl -o- https://raw.githubusercontent.com/creationix/nvm/v0.26.1/install.sh | bash

# TODO Install Node stable and make it default
# . ~/.nvm/nvm.sh
# nvm install stable
# nvm alias default stable

# Install RVM and Ruby stable
gpg --keyserver hkp://keys.gnupg.net --recv-keys 409B6B1796C275462A1703113804BB82D39DC0E3
curl -sSL https://get.rvm.io | bash -s stable --autolibs=enable --auto-dotfiles --ruby
echo "gem: --no-document" >> ~/.gemrc

# TODO Install gems
# . ~/.rvm/scripts/rvm
# gem install artoo
# gem install bundler
# gem install compass
# gem install rake
# gem install rails
# gem install rubocop
# gem install rubycritic
# gem install wordmove

# Install WPScan
# cd ~/bin &&
#   git clone https://github.com/wpscanteam/wpscan.git &&
#   cd wpscan
#   bundle install --without test

# Install Oh My Zsh
curl -L https://github.com/robbyrussell/oh-my-zsh/raw/master/tools/install.sh | sh

# Install Atom packages
apm install language-apache language-babel language-docker language-elixir language-elm language-generic-config language-haproxy language-haskell language-nginx language-rspec language-rust language-scala language-slim atom-alignment change-case custom-title git-plus highlight-column open-git-modified-files pinned-tabs rails-open-rspec rspec

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
git config --global core.editor vim
git config --global core.filemode false
git config --global merge.tool opendiff
git config --global push.default current
