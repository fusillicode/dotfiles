#!/usr/bin/env sh

# Install Xcode tools
xcode-select --install

# Setup Homebrew
ruby -e "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/uninstall)"
ruby -e "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install)"
brew update
brew doctor
brew cask doctor
brew tap caskroom/cask
brew tap caskroom/versions

# Install Homebrew apps
brew install asdf
brew install awscli
brew install ansible
brew install bat
brew tap codefresh-io/cli
brew install codefresh
brew install cpulimit
brew install exa
brew install fd
brew install git
brew install hadolint
brew tap heroku/brew && brew install heroku
# brew install ifstat # This was for the network.sh BitBar script...that I don't use it anymore ¯\_(ツ)_/¯
brew install htop
brew install imagemagick@6
brew install jq
brew install kube-ps1
brew install kubectx
brew install kustomize
brew install libpq && brew link libp --force
brew install librdkafka # It installs also `lz4`, `lzlib` & `zstd`
brew install mycli
brew install mysql-client # For Python `mysqlclient`
brew install openvpn # It installs also `lzo` & `pkcs11-helper`
brew install pgcli
brew install ripgrep
brew install siege
brew install stern
brew install txn2/tap/kubefwd
brew install watch
brew install zsh

# Install Homebrew-Cask apps
brew cask install appcleaner
brew cask install bitbar
brew cask install camunda-modeler
brew cask install chromedriver
brew cask install firefox
brew cask install google-chrome
brew cask install helm
brew cask install keybase
brew cask install league-of-legends
brew cask install libreoffice
brew cask install rcdefaultapp
brew cask install sfdx
brew cask install skype
brew cask install slack
brew cask install smcfancontrol
brew cask install spectacle
brew cask install telegram
brew cask install the-unarchiver
brew cask install transmission
brew cask install tunnelblick
brew cask install whatsapp

# Install docker-slim
curl -OL https://downloads.dockerslim.com/releases/1.26.1/dist_mac.zip && unzip dist_mac.zip && mv dist_mac/* ~/bin

# Install Oh My Zsh
sh -c "$(curl -fsSL https://raw.githubusercontent.com/robbyrussell/oh-my-zsh/master/tools/install.sh)"

# Configure asdf
echo -e "\n. $(brew --prefix asdf)/asdf.sh" >> ~/.zshrc
echo -e "\n. $(brew --prefix asdf)/etc/bash_completion.d/asdf.bash" >> ~/.zshrc

# Install asdf plugins
# brew install bdw-gc libevent llvm && brew link llvm --force # Don't know, seems not needed anymore...¯\_(ツ)_/¯
asdf plugin-add crystal https://github.com/marciogm/asdf-crystal.git
brew install autoconf wxmac
asdf plugin-add erlang https://github.com/asdf-vm/asdf-erlang.git
asdf plugin-add elixir https://github.com/asdf-vm/asdf-elixir.git
asdf plugin-add ruby https://github.com/asdf-vm/asdf-ruby.git
asdf plugin-add python https://github.com/asdf-vm/asdf-python.git
brew install gpg
asdf plugin-add nodejs https://github.com/asdf-vm/asdf-nodejs.git
bash ~/.asdf/plugins/nodejs/bin/import-release-team-keyring
# https://github.com/asdf-vm/asdf-nodejs#nvmrc-and-node-version-files
"legacy_version_file = yes" >> ~/.asdfrc
asdf plugin-add java https://github.com/halcyon/asdf-java.git

# Install Ruby gems
gem install --no-rdoc --no-ri \
  brakeman bundler-audit bundler cane compass consistency_fail fasterer html2slim license_finder loc_counter rails \
  rails-audit rails_best_practices rake reek ruby-lint rubocop rubycritic solargraph wordmove

# Install WpScan
mkdir -p ~/bin/ && ~/bin && git clone https://github.com/wpscanteam/wpscan.git && cd wpscan && bundle install --without test

# Install Elixir libs
mix local.hex
mix local.rebar
mix archive.install https://github.com/phoenixframework/archives/raw/master/phoenix_new.ez

# Install Erlang tools
# curl -OL https://github.com/erlanglab/erlangpl/releases/download/0.9.0/erlangpl.tar.gz && \
# tar -zxvf erlangpl.tar.gz && \
# rm -rf erlangpl.tar.gz && \
# mkdir -p ~/bin/ && \
# mv erlangpl ~/bin
# chmod +x ~/bin/*

# Install Python modules
# pip install ansible black boto boto3 ipython psycopg2

# Poetry
curl -sSL https://raw.githubusercontent.com/python-poetry/poetry/master/get-poetry.py | python

# Haskell
curl https://get-ghcup.haskell.org -sSf | sh

# Install Visual Studio Code Insiders extension
code --install-extension Shan.code-settings-sync

# Upgrade and cleanup brew stuff...
brew update && brew upgrade && brew cleanup -s && rm -rf ~/Library/Caches/Homebrew/*
brew doctor
brew cask doctor

# Set up git
git config --global core.editor vim
git config --global core.filemode false
git config --global merge.tool opendiff
git config --global push.default current
git config --global branch.autosetuprebase always
