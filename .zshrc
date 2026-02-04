# Path to your oh-my-zsh installation.
export ZSH="$HOME/.oh-my-zsh"

# Which plugins would you like to load?
# Standard plugins can be found in ~/.oh-my-zsh/plugins/*
# Custom plugins may be added to ~/.oh-my-zsh/custom/plugins/
# Add wisely, as too many plugins slow down shell startup.
plugins=(gitfast docker docker-compose kubectl)

. "$ZSH/oh-my-zsh.sh"

# Shell options
setopt AUTO_CD              # cd by typing directory name
setopt AUTO_PUSHD           # Push directories onto stack
setopt PUSHD_IGNORE_DUPS    # Don't push duplicates
setopt PUSHD_SILENT         # Don't print stack after pushd/popd
setopt EXTENDED_GLOB        # Extended glob patterns
setopt COMPLETE_IN_WORD     # Complete from both ends of word
setopt ALWAYS_TO_END        # Move cursor to end on completion

# Preferred editor
export EDITOR='nvim'

# PATH (deduplicated)
typeset -U path
path=(
  "$HOME/.local/bin"
  "/opt/homebrew/opt/openssl/bin"
  $path
)

# Rust
[ -e "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# Build flags (rdkafka & M1)
export OPENSSL_ROOT_DIR="/opt/homebrew/opt/openssl"
export LDFLAGS="-L/opt/homebrew/opt/openssl/lib -L/opt/homebrew/opt/llvm/lib"
export CPPFLAGS="-I/opt/homebrew/opt/openssl/include -I/opt/homebrew/opt/llvm/include"
export PKG_CONFIG_PATH="/opt/homebrew/opt/openssl/lib/pkgconfig"

# Mise (version manager)
eval "$(mise activate zsh)"

# Starship
eval "$(starship init zsh)"

# Aliases (must be before fzf history which uses 'h' alias)
. "$HOME/.zsh_aliases"

# Custom history with fzf
. "$HOME/.zsh-fzf-custom-history"
