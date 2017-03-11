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
brew tap beeftornado/rmtree
brew tap caskroom/cask
brew tap caskroom/versions

# Install Homebrew apps
brew install awscli
brew install beeftornado/rmtree/brew-rmtree
brew install chromedriver
brew install cputhrottle
brew install exercism
brew install fwup
brew install git
brew install gpg
brew install heroku-toolbelt
brew install hybridgroup/tools/gort
brew install imagemagick
brew install jq
brew install mackup
brew install mcrypt
brew install mysql
brew install mpv
brew install phantomjs
brew install qt
brew install siege
brew install squashfs
brew install zsh

# Install Homebrew-Cask
brew install caskroom/cask/brew-cask
brew update && brew upgrade brew-cask && brew cleanup -s && brew cask cleanup && rm -rf ~/Library/Caches/Homebrew/*

# Install Homebrew-Cask apps
brew cask install adium
brew cask install appcleaner
brew cask install atom-beta
brew cask install bitbar
brew cask install ccleaner
brew cask install dropbox
brew cask install firefox
brew cask install google-chrome
brew cask install java
brew cask install libreoffice
brew cask install rcdefaultapp
brew cask install skype
brew cask install slack
brew cask install spectacle
brew cask install the-unarchiver
brew cask install transmission
brew cask install yubico-authenticator

# Upgrade everything and remove outdated versions from the cellar
brew update && brew upgrade brew-cask && brew cleanup -s && brew cask cleanup && rm -rf ~/Library/Caches/Homebrew/*

# Install Oh My Zsh
curl -L https://github.com/robbyrussell/oh-my-zsh/raw/master/tools/install.sh | sh

# Install and configure asdf
git clone https://github.com/asdf-vm/asdf.git ~/.asdf --branch v0.2.1
asdf plugin-add clojure https://github.com/vic/asdf-clojure.git
asdf plugin-add elixir https://github.com/asdf-vm/asdf-elixir.git
asdf plugin-add elm https://github.com/vic/asdf-elm.git
asdf plugin-add erlang https://github.com/asdf-vm/asdf-erlang.git
asdf plugin-add golang https://github.com/kennyp/asdf-golang.git
asdf plugin-add haskell https://github.com/vic/asdf-haskell.git
asdf plugin-add php https://github.com/odarriba/asdf-php.git
asdf plugin-add postgres https://github.com/smashedtoatoms/asdf-postgres.git
asdf plugin-add redis https://github.com/smashedtoatoms/asdf-redis.git
asdf plugin-add ruby https://github.com/asdf-vm/asdf-ruby.git
asdf plugin-add rust https://github.com/code-lever/asdf-rust.git
asdf plugin-add scala https://github.com/mtatheonly/asdf-scala
# TODO: install asdf versions

# Install ECS deploy
mkdir -p ~/bin/
curl -L https://raw.githubusercontent.com/silinternational/ecs-deploy/master/ecs-deploy > ~/bin/ecs-deploy
# Install smc utility
curl -LO http://www.eidac.de/smcfancontrol/smcfancontrol_2_6.zip && \
unzip -d temp_dir_smc smcfancontrol_2_6.zip && \
yes | cp -f temp_dir_smc/smcFanControl.app/Contents/Resources/smc ~/bin/smc && \
rm -rf temp_dir_smc smcfancontrol_2_6.zip
chmod +x ~/bin/*
# This should already be handled by other stuff
# echo "export PATH=/usr/local/bin:/usr/local/sbin:$PATH:/Users/`whoami`/bin" >> ~/.profile

# TODO: install gems
# gem install brakeman bundler-audit bundler cane compass consistency_fail html2slim license_finder rails rails-audit rails_best_practices rake reek ruby-lint rubocop rubycritic wordmove

# TODO: install elixir libs
# mix local.hex
# mix local.rebar
# mix archive.install https://github.com/phoenixframework/archives/raw/master/phoenix_new.ez
# mix archive.install https://github.com/nerves-project/archives/raw/master/nerves_bootstrap.ez

# TODO: install python modules (ansible and its requirements)
# pip install ansible boto boto3 psycopg2

# Install WPScan
# cd ~/bin &&
#   git clone https://github.com/wpscanteam/wpscan.git &&
#   cd wpscan
#   bundle install --without test

# Install Atom packages
apm install atom-alignment atom-beautify change-case custom-title export-html git-tools highlight-column language-babel language-docker language-elixir language-elm language-haml language-haskell language-rspec language-rust language-scala language-slim lines monokai-blackboard open-git-modified-files pinned-tabs rails-transporter ruby-test trailing-spaces

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
