#!/usr/bin/env sh

# Install Xcode tools
xcode-select --install

# Setup Homebrew
ruby -e "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/uninstall)"
ruby -e "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install)"
brew update
brew doctor
brew cask doctor
brew prune
brew tap caskroom/cask
brew tap caskroom/versions

# Install Homebrew apps
brew install awscli
brew install git
brew install helm
brew install heroku
brew install ifstat
brew install imagemagick@6
brew install jq
brew install kube-ps1
brew install kubectx
brew install kustomize
brew install mycli
brew install pgcli
brew install ripgrep
brew install siege
brew install txn2/tap/kubefwd
brew install zsh

# Install Homebrew-Cask apps
brew cask install appcleaner
brew cask install bitbar
brew cask install camunda-modeler
brew cask install chromedriver
brew cask install firefox
brew cask install google-backup-and-sync
brew cask install google-chrome
brew cask install keybase
brew cask install libreoffice
brew cask install rcdefaultapp
brew cask install sfdx
brew cask install skype
brew cask install slack
brew cask install spectacle
brew cask install telegram
brew cask install the-unarchiver
brew cask install transmission
brew cask install tunnelblick
brew cask install whatsapp

# Install Oh My Zsh
curl -L https://github.com/robbyrussell/oh-my-zsh/raw/master/tools/install.sh | sh

# Install smc utility
curl -LO http://www.eidac.de/smcfancontrol/smcfancontrol_2_6.zip && \
unzip -d temp_dir_smc smcfancontrol_2_6.zip && \
mkdir -p ~/bin/ && \
yes | cp -f temp_dir_smc/smcFanControl.app/Contents/Resources/smc ~/bin/smc && \
rm -rf temp_dir_smc smcfancontrol_2_6.zip
chmod +x ~/bin/*

# Install and configure asdf
git clone https://github.com/asdf-vm/asdf.git ~/.asdf --branch v0.7.3
brew install coreutils automake autoconf openssl libyaml readline libxslt libtool unixodbc unzip curl
echo -e '\n. $HOME/.asdf/asdf.sh' >> ~/.zshrc
echo -e '\n. $HOME/.asdf/completions/asdf.bash' >> ~/.zshrc

# Install asdf plugins
brew install bdw-gc libevent llvm
brew link llvm --force
asdf plugin-add crystal https://github.com/marciogm/asdf-crystal.git
brew install wxmac
asdf plugin-add erlang https://github.com/asdf-vm/asdf-erlang.git
asdf plugin-add elixir https://github.com/asdf-vm/asdf-elixir.git
asdf plugin-add ruby https://github.com/asdf-vm/asdf-ruby.git

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

# Install Visual Studio Code Insiders extensions
code --install-extensions Atishay-Jain.All-Autocomplete
code --install-extensions CraigMaslowski.erb
code --install-extensions DotJoshJohnson.xml
code --install-extensions HaaLeo.timing
code --install-extensions JakeBecker.elixir-ls
code --install-extensions SirTori.indenticator
code --install-extensions VisualStudioExptTeam.vscodeintellicode
code --install-extensions alefragnani.project-manager
code --install-extensions bungcip.better-toml
code --install-extensions castwide.solargraph
code --install-extensions christian-kohler.path-intellisense
code --install-extensions dakara.transformer
code --install-extensions dbaeumer.vscode-eslint
code --install-extensions eamodio.gitlens
code --install-extensions eriklynd.json-tools
code --install-extensions faustinoaq.crystal-lang
code --install-extensions gayanhewa.referenceshelper
code --install-extensions iampeterbanjo.elixirlinter
code --install-extensions imperez.smarty
code --install-extensions joaompinto.asciidoctor-vscode
code --install-extensions justusadam.language-haskell
code --install-extensions karunamurti.haml
code --install-extensions karunamurti.rspec-snippets
code --install-extensions kevinkyang.auto-comment-blocks
code --install-extensions kumar-harsh.graphql-for-vscode
code --install-extensions mauve.terraform
code --install-extensions misogi.ruby-rubocop
code --install-extensions mitchdenny.ecdc
code --install-extensions ms-azuretools.vscode-docker
code --install-extensions ms-kubernetes-tools.vscode-kubernetes-tools
code --install-extensions ms-python.python
code --install-extensions ms-vscode.vscode-typescript-tslint-plugin
code --install-extensions nextfaze.json-parse-stringify
code --install-extensions nwolverson.language-purescript
code --install-extensions pgourlain.erlang
code --install-extensions quicktype.quicktype
code --install-extensions rebornix.ruby
code --install-extensions redhat.java
code --install-extensions redhat.vscode-yaml
code --install-extensions robert.ruby-snippet
code --install-extensions robinbentley.sass-indented
code --install-extensions rust-lang.rust
code --install-extensions sbrink.elm
code --install-extensions scala-lang.scala
code --install-extensions scalameta.metals
code --install-extensions shanoor.vscode-nginx
code --install-extensions shardulm94.trailing-spaces
code --install-extensions sianglim.slim
code --install-extensions sleistner.vscode-fileutils
code --install-extensions sporto.rails-go-to-spec
code --install-extensions steve8708.Align
code --install-extensions stkb.rewrap
code --install-extensions technosophos.vscode-helm
code --install-extensions tomoki1207.selectline-statusbar
code --install-extensions vscjava.vscode-java-debug
code --install-extensions vscjava.vscode-java-dependency
code --install-extensions vscjava.vscode-java-pack
code --install-extensions vscjava.vscode-java-test
code --install-extensions vscjava.vscode-maven
code --install-extensions webfreak.debug
code --install-extensions wholroyd.jinja
code --install-extensions wmaurer.change-case
code --install-extensions yuce.erlang-otp

# Upgrade and cleanup brew stuff...
brew update && brew upgrade && brew cleanup -s && rm -rf ~/Library/Caches/Homebrew/*
brew doctor
brew cask doctor
brew prune

# Set up git
git config --global core.editor vim
git config --global core.filemode false
git config --global merge.tool opendiff
git config --global push.default current
git config --global branch.autosetuprebase always
