# My zsh theme based on smt.zsh-theme, which is based on dogenpunk by Matthew Nelson ;P

MODE_INDICATOR="%{$fg_bold[red]%}❮%{$reset_color%}%{$fg_bold[red]%}❮❮%{$reset_color%}"

ZSH_THEME_GIT_PROMPT_PREFIX=" "
ZSH_THEME_GIT_PROMPT_SUFFIX="%{$reset_color%}"
ZSH_THEME_GIT_PROMPT_DIRTY=" "
ZSH_THEME_GIT_PROMPT_AHEAD=" "
ZSH_THEME_GIT_PROMPT_CLEAN="%{$fg_bold[green]%} ✓ %{$reset_color%}"

ZSH_THEME_GIT_PROMPT_ADDED="%{$fg_bold[green]%}✚ %{$reset_color%}"
ZSH_THEME_GIT_PROMPT_MODIFIED="%{$fg_bold[blue]%}! %{$reset_color%}"
ZSH_THEME_GIT_PROMPT_DELETED="%{$fg_bold[red]%}- %{$reset_color%}"
ZSH_THEME_GIT_PROMPT_RENAMED="%{$fg_bold[magenta]%}> %{$reset_color%}"
ZSH_THEME_GIT_PROMPT_UNMERGED="%{$fg_bold[yellow]%} #%{$reset_color%}"
ZSH_THEME_GIT_PROMPT_UNTRACKED="%{$fg_bold[cyan]%}? %{$reset_color%}"

# Format for git_prompt_long_sha() and git_prompt_short_sha()
ZSH_THEME_GIT_PROMPT_SHA_BEFORE="%{$fg_bold[yellow]%}"
ZSH_THEME_GIT_PROMPT_SHA_AFTER="%{$reset_color%}"

# For kube-ps1
KUBE_PS1_PREFIX=
KUBE_PS1_SEPARATOR=
KUBE_PS1_SUFFIX=
KUBE_PS1_SYMBOL_ENABLE=false
source "/opt/homebrew/opt/kube-ps1/share/kube-ps1.sh"
PS1='$(kube_ps1)'$PS1

return_status() {
  if [[ $? -ne 0 ]]; then
    echo "%{$fg_bold[red]%}$(launch_time)%{$reset_color%}";
  else
    echo "%{$fg_bold[green]%}$(launch_time)%{$reset_color%}";
  fi
}

prompt_char() {
  git branch >/dev/null 2>/dev/null && echo "%{$fg_bold[cyan]%}±%{$reset_color%}" && return
  echo "%{$fg_bold[cyan]%}○%{$reset_color%}"
}

git_tag() {
  tag=$(git describe --tags --exact-match 2> /dev/null)
  if [[ -z "${tag// }" ]]; then
    echo ""
  else
    echo " %{$fg_bold[magenta]%}%{$tag%}"
  fi
}

launch_time() {
  echo "%*"
}

path() {
  echo "%{$fg_bold[cyan]%}%~%{$reset_color%}"
}

PROMPT=$'
$(return_status) $(path)$(git_prompt_info)$(git_prompt_status)$(git_prompt_short_sha)$(git_tag) $(kube_ps1)
$(prompt_char) '
