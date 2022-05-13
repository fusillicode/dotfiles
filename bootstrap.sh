#!/usr/bin/env sh

# Install Xcode tools
xcode-select --install

# Setup Homebrew
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/uninstall.sh)"
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install.sh)"
brew update
brew doctor --verbose

# Install Homebrew apps
brew install act
brew install asdf
brew install awscli
brew install ansible
brew install atuin
# brew tap codefresh-io/cli && brew install codefresh
brew install cpulimit
brew install git
brew install gh
brew install hadolint
brew install hashicorp/tap/terraform-ls
brew tap heroku/brew && brew install heroku
# brew install ifstat # This was for the network.sh BitBar script...that I don't use it anymore ¯\_(ツ)_/¯
brew install htop
brew install imagemagick@6
brew install jq
brew install jsonnet # For VSCode Jsonnet extension
brew install kube-ps1
brew install kubectx
brew install kustomize
brew install lftp
brew install libpq && brew link libpq --force
brew install librdkafka # It also installs `lz4`, `lzlib` & `zstd`
brew install mycli # For Python `mysqlclient`
# brew install openvpn # It also installs `lzo` & `pkcs11-helper`
brew install ripgrep
brew install stern
brew install tmux
brew install txn2/tap/kubefwd
brew install vegeta
brew install watch
brew install yq
brew install zsh

# Install Homebrew-Cask apps
brew install homebrew/cask/docker
brew install android-file-transfer
brew install appcleaner
brew install bitbar
# brew install camunda-modeler
brew install chromedriver
brew install discord
brew install firefox
brew install google-chrome
# brew install helm
brew install http-toolkit
brew install league-of-legends
# brew install libreoffice
brew install rectangle
# brew install sfdx
brew install skype
brew install slack
brew install smcfancontrol
brew install telegram
brew install the-unarchiver
brew install transmission --cask
brew install tunnelblick
brew install vlc
brew install whatsapp

# Install docker-slim
# curl -OL https://downloads.dockerslim.com/releases/1.26.1/dist_mac.zip && unzip dist_mac.zip && mv dist_mac/* ~/bin

# Install Oh My Zsh
sh -c "$(curl -fsSL https://raw.githubusercontent.com/robbyrussell/oh-my-zsh/master/tools/install.sh)"

# Install rustup ❤️
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Rust CLI apps ❤️
cargo install --force cargo-make
cargo install sqlx-cli

# Configure asdf
echo -e "\n. $(brew --prefix asdf)/asdf.sh" >> ~/.zshrc
echo -e "\n. $(brew --prefix asdf)/etc/bash_completion.d/asdf.bash" >> ~/.zshrc

# Install asdf plugins
# brew install bdw-gc libevent llvm && brew link llvm --force # Don't know, seems not needed anymore...¯\_(ツ)_/¯
# asdf plugin-add crystal https://github.com/marciogm/asdf-crystal.git
brew install autoconf wxwidgets
export KERL_CONFIGURE_OPTIONS="--without-javac --with-ssl=$(brew --prefix openssl@1.1)"
asdf plugin-add erlang https://github.com/asdf-vm/asdf-erlang.git
asdf install erlang latest
asdf plugin-add elixir https://github.com/asdf-vm/asdf-elixir.git
asdf install elixir latest
# asdf plugin-add ruby https://github.com/asdf-vm/asdf-ruby.git
asdf plugin-add python
asdf install python latest
# brew install gpg && asdf plugin-add nodejs https://github.com/asdf-vm/asdf-nodejs.git && bash ~/.asdf/plugins/nodejs/bin/import-release-team-keyring
# https://github.com/asdf-vm/asdf-nodejs#nvmrc-and-node-version-files
# "legacy_version_file = yes" >> ~/.asdfrc
# asdf plugin-add java https://github.com/halcyon/asdf-java.git
# asdf plugin-add terraform https://github.com/Banno/asdf-hashicorp.git

# Install Ruby gems
gem install --no-rdoc --no-ri \
  brakeman bundler-audit bundler cane compass consistency_fail fasterer html2slim license_finder loc_counter rails \
  rails-audit rails_best_practices rake reek ruby-lint rubocop rubycritic solargraph wordmove

# Install WpScan
mkdir -p ~/bin/ && ~/bin && git clone https://github.com/wpscanteam/wpscan.git && cd wpscan && bundle install --without test

# Install Elixir libs
mix local.hex
mix local.rebar
mix archive.install hex phx_new 1.5.8

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

# Go stuff...for DevOps stuff...
# go get -u github.com/grafana/tanka/cmd/tk
# go get -u github.com/jsonnet-bundler/jsonnet-bundler/cmd/jb

# Haskell
curl https://get-ghcup.haskell.org -sSf | sh

# Upgrade and cleanup brew stuff...
brew update && brew upgrade && brew cleanup -s && rm -rf ~/Library/Caches/Homebrew/*
brew doctor

# Twitch miner
pip install Twitch-Channel-Points-Miner-v2
