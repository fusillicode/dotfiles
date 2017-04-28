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
brew install chromedriver
brew install exercism
brew install git
brew install heroku
brew install imagemagick@6
brew install mackup
brew install mysql
brew install mpv
brew install phantomjs
brew install siege
brew install zsh

# Install Homebrew-Cask apps
brew cask install adium
brew cask install appcleaner
brew cask install bitbar
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

# Install Oh My Zsh
curl -L https://github.com/robbyrussell/oh-my-zsh/raw/master/tools/install.sh | sh

# Install ECS deploy
brew install jq
mkdir -p ~/bin/
curl -L https://raw.githubusercontent.com/silinternational/ecs-deploy/master/ecs-deploy > ~/bin/ecs-deploy

# Install smc utility
curl -LO http://www.eidac.de/smcfancontrol/smcfancontrol_2_6.zip && \
unzip -d temp_dir_smc smcfancontrol_2_6.zip && \
yes | cp -f temp_dir_smc/smcFanControl.app/Contents/Resources/smc ~/bin/smc && \
rm -rf temp_dir_smc smcfancontrol_2_6.zip
chmod +x ~/bin/*

# Install and configure asdf
git clone https://github.com/asdf-vm/asdf.git ~/.asdf --branch v0.3.0
brew install coreutils automake autoconf openssl libyaml readline libxslt libtool unixodbc
echo -e '\n. $HOME/.asdf/asdf.sh' >> ~/.zshrc
echo -e '\n. $HOME/.asdf/completions/asdf.bash' >> ~/.zshrc

# Install asdf plugins
asdf plugin-add ruby https://github.com/asdf-vm/asdf-ruby.git
brew install wxmac
asdf plugin-add erlang https://github.com/asdf-vm/asdf-erlang.git
asdf plugin-add elixir https://github.com/asdf-vm/asdf-elixir.git
asdf plugin-add redis https://github.com/smashedtoatoms/asdf-redis.git
asdf plugin-add postgres https://github.com/smashedtoatoms/asdf-postgres.git
asdf plugin-add nodejs https://github.com/asdf-vm/asdf-nodejs.git
brew install gpg
chmod 700 ~/.gnupg
chmod 600 ~/.gnupg/*
bash ~/.asdf/plugins/nodejs/bin/import-release-team-keyring
# asdf plugin-add clojure https://github.com/vic/asdf-clojure.git
# asdf plugin-add crystal https://github.com/marciogm/asdf-crystal.git
# asdf plugin-add elm https://github.com/vic/asdf-elm.git
# asdf plugin-add golang https://github.com/kennyp/asdf-golang.git
# asdf plugin-add haskell https://github.com/vic/asdf-haskell.git
# asdf plugin-add php https://github.com/odarriba/asdf-php.git
# asdf plugin-add python https://github.com/tuvistavie/asdf-python.git
# brew install gcc
# asdf plugin-add riak https://github.com/smashedtoatoms/asdf-riak
# asdf plugin-add rust https://github.com/code-lever/asdf-rust.git
# asdf plugin-add sbt https://github.com/lerencao/asdf-sbt
# asdf plugin-add scala https://github.com/mtatheonly/asdf-scala
# asdf plugin-add swift https://github.com/fcrespo82/asdf-swift

# Install gems
gem install --no-rdoc --no-ri brakeman bundler-audit bundler cane compass consistency_fail html2slim license_finder rails rails-audit rails_best_practices rake reek ruby-lint rubocop rubycritic wordmove

# Install wpscan
mkdir -p ~/bin/ && ~/bin && git clone https://github.com/wpscanteam/wpscan.git && cd wpscan && bundle install --without test

# Install elixir libs
mix local.hex
mix local.rebar
mix archive.install https://github.com/phoenixframework/archives/raw/master/phoenix_new.ez
brew install coreutils fwup squashfs
mix archive.install https://github.com/nerves-project/archives/raw/master/nerves_bootstrap.ez

# Install python modules
pip install ansible boto boto3 psycopg2

# Install Visual Studio code
code --install-extension

# Upgrade and cleanup brew stuff...
brew update && brew upgrade && brew cleanup -s && brew cask cleanup && rm -rf ~/Library/Caches/Homebrew/*
brew doctor
brew prune

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
git config --global branch.autosetuprebase always