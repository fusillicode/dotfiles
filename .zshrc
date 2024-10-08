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

### MANAGED BY RANCHER DESKTOP START (DO NOT EDIT)
export PATH="/Users/gianlucarandazzo/.rd/bin:$PATH"
### MANAGED BY RANCHER DESKTOP END (DO NOT EDIT)

# Which plugins would you like to load?
# Standard plugins can be found in ~/.oh-my-zsh/plugins/*
# Custom plugins may be added to ~/.oh-my-zsh/custom/plugins/
# Example format: plugins=(rails git textmate ruby lighthouse)
# Add wisely, as too many plugins slow down shell startup.
plugins=(git gitfast docker docker-compose kubectl kube-ps1)

. "$ZSH/oh-my-zsh.sh"

# User configuration

# export MANPATH="/usr/local/man:$MANPATH"

# You may need to manually set your language environment
# export LANG=en_US.UTF-8

# Preferred editor for local and remote sessions
if [[ -n $SSH_CONNECTION ]]; then
  export EDITOR='nvim'
else
  export EDITOR='nvim'
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
. "$HOME/.zshenv"


# Atuin 🐢
eval "$(atuin init zsh)"

# Mise 😎
eval "$(mise activate zsh)"

# My bins
export PATH=$HOME/.local/bin:$PATH

# 🥲 https://wezfurlong.org/wezterm/faq.html#how-do-i-enable-undercurl-curly-underlines
export TERM=wezterm

# Helix from the source! 😎
export HELIX_RUNTIME=~/.dev-tools/helix/runtime

# K8S
export KUBECONFIG=~/.kube/config:~/.kube/config.qa:~/.kube/config.prod

# Rust
[ -e "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# Haskell
[ -e "$HOME/.ghcup/env" ] && . "$HOME/.ghcup/env"

# Elixir & Erlang
export ERL_AFLAGS="-kernel shell_history enabled -kernel shell_history_file_bytes 1024000"

# ...Go...
export PATH="$HOME/go/bin:$PATH"

# ...Poetry...
[ -e "$HOME/.poetry/env" ] && . "$HOME/.poetry/env"
export PATH="$HOME/.poetry/bin:$PATH"

# Lira 🥲
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"

# rdkafka & M1 🥲
export PATH="/opt/homebrew/opt/openssl/bin:$PATH"
export OPENSSL_ROOT_DIR="/opt/homebrew/opt/openssl"
export LDFLAGS="-L/opt/homebrew/opt/openssl/lib -L/opt/homebrew/opt/llvm/lib"
export CPPFLAGS="-I/opt/homebrew/opt/openssl/include -I/opt/homebrew/opt/llvm/include"
export PKG_CONFIG_PATH="/opt/homebrew/opt/openssl/lib/pkgconfig"

# FFS PHP...
export PATH="/opt/homebrew/opt/m4/bin:$PATH"

# FFS...k8s
export PATH="${KREW_ROOT:-$HOME/.krew}/bin:$PATH"
