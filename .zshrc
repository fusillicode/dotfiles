# Cache directory
_zsh_cache="$HOME/.cache/zsh"
[[ -d "$_zsh_cache" ]] || mkdir -p "$_zsh_cache"

# Completions (git, docker from Homebrew; kubectl, docker-compose cached)
fpath=(/opt/homebrew/share/zsh/site-functions $fpath)

if (( $+commands[kubectl] )); then
  _kubectl_comp="$_zsh_cache/_kubectl"
  if [[ ! -f "$_kubectl_comp" || "$(command -v kubectl)" -nt "$_kubectl_comp" ]]; then
    kubectl completion zsh > "$_kubectl_comp"
  fi
  fpath=("$_zsh_cache" $fpath)
fi

if (( $+commands[docker] )); then
  _docker_compose_comp="$_zsh_cache/_docker-compose"
  if [[ ! -f "$_docker_compose_comp" || "$(command -v docker)" -nt "$_docker_compose_comp" ]]; then
    docker compose completion zsh > "$_docker_compose_comp" 2>/dev/null || true
  fi
fi

autoload -Uz compinit
if [[ -n ~/.zcompdump(#qN.mh+24) ]]; then
  compinit
else
  compinit -C
fi
[[ ~/.zcompdump -nt ~/.zcompdump.zwc ]] && zcompile ~/.zcompdump

# Completion styling
zstyle ':completion:*' menu selection
zstyle ':completion:*' matcher-list 'm:{a-z}={A-Z}'
zstyle ':completion:*' list-colors ${(s.:.)LS_COLORS}
zstyle ':completion:*' squeeze-slashes true
zstyle ':completion:*' file-sort modification
_comp_options+=(globdots)

# Terminal title (shows current directory or running command)
function _set_title() { print -Pn "\e]0;$1\a"; }
function precmd() { _set_title "%~"; }
function preexec() { _set_title "$1"; }

# Line editing (emacs mode; zsh defaults to vi when EDITOR contains "vi")
bindkey -e

# Subword navigation
WORDCHARS='*?~&;!#$%^'

# Shell options
setopt ALWAYS_TO_END        # Move cursor to end on completion
setopt AUTO_CD              # cd by typing directory name
setopt AUTO_PUSHD           # Push directories onto stack
setopt COMPLETE_IN_WORD     # Complete from both ends of word
setopt EXTENDED_GLOB        # Extended glob patterns
setopt INTERACTIVE_COMMENTS # Allow comments in interactive shell
setopt NO_BEEP              # Disable terminal beep
setopt PUSHD_IGNORE_DUPS    # Don't push duplicates
setopt PUSHD_SILENT         # Don't print stack after pushd/popd

# Environment
export EDITOR='nvim'

# PATH (deduplicated, skip non-existent)
typeset -U path
path=(
  ${HOME}/.local/bin(N)
  ${HOME}/.cargo/bin(N)
  /opt/homebrew/opt/openssl/bin(N)
  $path
)

# Build flags (rdkafka & M1)
export OPENSSL_ROOT_DIR="/opt/homebrew/opt/openssl"
export LDFLAGS="-L/opt/homebrew/opt/openssl/lib -L/opt/homebrew/opt/llvm/lib"
export CPPFLAGS="-I/opt/homebrew/opt/openssl/include -I/opt/homebrew/opt/llvm/include"
export PKG_CONFIG_PATH="/opt/homebrew/opt/openssl/lib/pkgconfig"

# Mise (cached)
_mise_cache="$_zsh_cache/mise.zsh"
if [[ ! -f "$_mise_cache" || "$(command -v mise)" -nt "$_mise_cache" ]]; then
  mise activate zsh > "$_mise_cache"
fi
source "$_mise_cache"

# Starship (cached)
_starship_cache="$_zsh_cache/starship.zsh"
if [[ ! -f "$_starship_cache" || "$(command -v starship)" -nt "$_starship_cache" ]]; then
  starship init zsh > "$_starship_cache"
fi
source "$_starship_cache"

# Aliases (must be before fzf history which uses 'h' alias)
. "$HOME/.zsh_aliases"

# Custom history with fzf
. "$HOME/.zsh-fzf-custom-history"

# Private config
[[ -f "$HOME/.zshrc.local" ]] && . "$HOME/.zshrc.local"

# Compile zsh scripts for faster loading
[[ ~/.zshrc -nt ~/.zshrc.zwc ]] && zcompile ~/.zshrc
[[ ~/.zsh_aliases -nt ~/.zsh_aliases.zwc ]] && zcompile ~/.zsh_aliases

# Exit with 0 if everything's fine
true
