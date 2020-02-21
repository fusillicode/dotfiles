# If you come from bash you might have to change your $PATH.
# export PATH=$HOME/bin:/usr/local/bin:$PATH

# Path to your oh-my-zsh installation.
export ZSH="$HOME/.oh-my-zsh"

# Set name of the theme to load --- if set to "random", it will
# load a random theme each time oh-my-zsh is loaded, in which case,
# to know which specific one was loaded, run: echo $RANDOM_THEME
# See https://github.com/robbyrussell/oh-my-zsh/wiki/Themes
ZSH_THEME="my-zsh"

# Set list of themes to pick from when loading at random
# Setting this variable when ZSH_THEME=random will cause zsh to load
# a theme from this variable instead of looking in ~/.oh-my-zsh/themes/
# If set to an empty array, this variable will have no effect.
# ZSH_THEME_RANDOM_CANDIDATES=( "robbyrussell" "agnoster" )

# Uncomment the following line to use case-sensitive completion.
# CASE_SENSITIVE="true"

# Uncomment the following line to use hyphen-insensitive completion.
# Case-sensitive completion must be off. _ and - will be interchangeable.
# HYPHEN_INSENSITIVE="true"

# Uncomment the following line to disable bi-weekly auto-update checks.
# DISABLE_AUTO_UPDATE="true"

# Uncomment the following line to automatically update without prompting.
# DISABLE_UPDATE_PROMPT="true"

# Uncomment the following line to change how often to auto-update (in days).
# export UPDATE_ZSH_DAYS=13

# Uncomment the following line if pasting URLs and other text is messed up.
# DISABLE_MAGIC_FUNCTIONS=true

# Uncomment the following line to disable colors in ls.
# DISABLE_LS_COLORS="true"

# Uncomment the following line to disable auto-setting terminal title.
# DISABLE_AUTO_TITLE="true"

# Uncomment the following line to enable command auto-correction.
# ENABLE_CORRECTION="true"

# Uncomment the following line to display red dots whilst waiting for completion.
# COMPLETION_WAITING_DOTS="true"

# Uncomment the following line if you want to disable marking untracked files
# under VCS as dirty. This makes repository status check for large repositories
# much, much faster.
# DISABLE_UNTRACKED_FILES_DIRTY="true"

# Uncomment the following line if you want to change the command execution time
# stamp shown in the history command output.
# You can set one of the optional three formats:
# "mm/dd/yyyy"|"dd.mm.yyyy"|"yyyy-mm-dd"
# or set a custom format using the strftime format specifications,
# see 'man strftime' for details.
# HIST_STAMPS="mm/dd/yyyy"

# Would you like to use another custom folder than $ZSH/custom?
# ZSH_CUSTOM=/path/to/new-custom-folder

# Which plugins would you like to load?
# Standard plugins can be found in ~/.oh-my-zsh/plugins/*
# Custom plugins may be added to ~/.oh-my-zsh/custom/plugins/
# Example format: plugins=(rails git textmate ruby lighthouse)
# Add wisely, as too many plugins slow down shell startup.
plugins=(git gitfast kubectl)

. $ZSH/oh-my-zsh.sh

# User configuration

# export MANPATH="/usr/local/man:$MANPATH"

# You may need to manually set your language environment
# export LANG=en_US.UTF-8

# Preferred editor for local and remote sessions
if [[ -n $SSH_CONNECTION ]]; then
  export EDITOR='vim'
else
  export EDITOR='code-insiders'
fi

# Compilation flags
# export ARCHFLAGS="-arch x86_64"

# Set personal aliases, overriding those provided by oh-my-zsh libs,
# plugins, and themes. Aliases can be placed here, though oh-my-zsh
# users are encouraged to define aliases within the ZSH_CUSTOM folder.
# For a full list of active aliases, run `alias`.
#
# Example aliases
# alias zshconfig="mate ~/.zshrc"
# alias ohmyzsh="mate ~/.oh-my-zsh"

# Short is better 🥞
alias code="code-insiders"
alias h="history -i"
alias ls="exa --sort=modified"
alias j="jq . -c"
alias jl="jq . "
alias cf="codefresh"
alias g-commit-nuke="git commit --amend --no-edit && git push --force"

# Easy Git
g-tag-nuke () {
  g tag -f $1 && g push origin $1 -f
}
g-tags () {
  g --no-pager tag
  g ls-remote --tags
}

# Easy CF
cf-list () {
  cf get builds ${1:+--status=$1} --select-columns id,repository,pipeline-name,branch,status
}
cf-log () {
  cf get builds ${2:+--status=$2} --select-columns id,repository,pipeline-name,branch,status | \
  rg "([^\s]*)\s.*$1.*" -r '$1' | head -n 1 | xargs -I {} codefresh logs $3 {}
}

# Easy K8S
k-sec-list () {
  k get secrets -oname ${1:+--namespace=$1}
}
k-sec-yaml () {
  k get secrets -oname ${2:+--namespace=$2} | \
  rg "secret/(.*$1.*)" -r '$1' | xargs -I {} kubectl get secret {} -oyaml
}
k-sec-decode () {
  k get secrets -oname ${3:+--namespace=$3} | \
  rg "secret/(.*$2.*)" -r '$1' | xargs -I {} kubectl get secret {} -oyaml | \
  rg "\s+(.*$1.*):\s+(.*)" -r '$1:$2' | \
  while read kv
  do
    dv=$(echo $kv | rg ".*:(.*)" -r '$1' | base64 -D)
    k=$(echo $kv | rg "(.*):.*" -r '$1')
    echo $k $dv
  done
}
k-cron-suspend () {
  k patch cronjobs $1 --patch '{"spec": {"suspend": '"$2"'}}'
}
k-cron-restart () {
  maybe_namespace=${2:+--namespace=$2}
  k get cronjobs $1 $maybe_namespace --export -oyaml > foo.yaml
  k delete -f $1 $maybe_namespace --ignore-not-found
  k apply -f $maybe_namespace /tmp/cron_job.yaml
}
k-depl-scale () {
  k patch deployment $1 --patch '{"spec": {"replicas": '"$2"'}}'
}

# My local `~/bin` "stuff" :P
export PATH=$HOME/bin:$PATH

# `brew link imagemagick@6` suggestion ¯\_(ツ)_/¯
export PATH="/usr/local/opt/imagemagick@6:$PATH"

# Crystal 0.26.1 + macOS Mojave
export PKG_CONFIG_PATH=$PKG_CONFIG_PATH:/usr/local/opt/openssl/lib/pkgconfig

# Scala...
export PATH="/usr/local/opt/sbt@0.13/bin:$PATH"

# K8S
export KUBECONFIG=~/.kube/config:~/.kube/config.qa:~/.kube/config.prod

# Elixir & Erlang
export ERL_AFLAGS="-kernel shell_history enabled -kernel shell_history_file_bytes 1024000"

# Rust
[ -e "$HOME/.cargo/env" ] && . $HOME/.cargo/env

# Haskell
[ -e "$HOME/.ghcup/env" ] && . $HOME/.ghcup/env

# ...Java...
[ -e "$HOME/.asdf/plugins/java/set-java-home.sh" ] && . $HOME/.asdf/plugins/java/set-java-home.sh

# `asdf` installation suggestion ¯\_(ツ)_/¯
[ -e /usr/local/opt/asdf/asdf.sh ] && . /usr/local/opt/asdf/asdf.sh
[ -e /usr/local/opt/asdf/etc/bash_completion.d/asdf.bash ] && . /usr/local/opt/asdf/etc/bash_completion.d/asdf.bash

# ...Poetry...
[ -e "$HOME/.poetry/env" ] && source $HOME/.poetry/env
