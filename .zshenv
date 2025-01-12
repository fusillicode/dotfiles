alias nv='nvim'
alias code='code-insiders'
alias h='atuin history list'
alias l='ls -llAtrh'
alias j='jq . -c'
alias jl='jq . '
alias sorce='f() { set -a && source "$@" && set +a; }; f'
alias gs='git status'
alias gidf='git diff'
alias gcnoke='git commit --amend --no-edit --no-verify --allow-empty'
alias gcnuke='git commit --amend --no-edit --no-verify --allow-empty && git push --force-with-lease --no-verify'
alias gtnuke='f() { git tag -f "$1" && git push origin "$1" -f }; f'
alias gbrr='git for-each-ref --sort=committerdate refs/heads/ --format="%(HEAD) %(color:yellow)%(refname:short)%(color:reset) %(color:red)%(objectname:short)%(color:reset) - %(contents:subject) %(color:green)(%(committerdate:relative))%(color:reset) %(color:blue)<%(authorname)>%(color:reset)"'
alias gdh='git diff HEAD~1'
alias gfp='git fetch --all --prune && git pull origin "$(git_current_branch)"'
alias gt='git --no-pager tag && git ls-remote --tags'
alias gm='f() { git commit -m "$*" }; f'
alias glod='f() { git --no-pager log --graph --pretty="%Cred%h%Creset%C(auto)%d%Creset %s %Cgreen(%ad) %C(bold blue)<%an>%Creset" -n "${1:-7}"; }; f'
alias kdebian='kubectl exec -it debian -- bash || kubectl run debian --image=debian:latest --rm -it --restart=Never --command --'
alias klfir='f() { kubectl get pods | rg "$1" | head -n 1 | rg "^(\S*).*" -r '\''$1'\'' | xargs -I {} kubectl logs -f {} "$2" }; f'
alias kseclist='f() { kubectl get secrets -oname ${1:+--namespace=$1} }; f'
alias ksecyaml='f() { kubectl get secrets -oname ${2:+--namespace=$2} | rg "secret/(.*$1.*)" -r '\''$1'\'' | xargs -I {} kubectl get secret {} -oyaml }; f'
alias kcronsus='f() { kubectl patch cronjobs "$1" --patch '\''{"spec": {"suspend": '\''"$2"'\''}}'\'' }; f'
alias kdeplscale='f() { kubectl patch deployment "$1" --patch '\''{"spec": {"replicas": '\''"$2"'\''}}'\'' }; f'
alias kdelerrpod='f() { kubectl get pods | rg "(\S+).*Error.*" -r '\''$1'\'' | xargs -I {} kubectl delete pod {} }; f'
alias pg_copy_table='f() { pg_dump -a -t "$1" "$2" | psql "$3" }; f'
alias carbo='cargo'
alias cmc='cargo make clippy'
alias cmch='cargo make check'
alias cmt='f() { cargo make test "$*" }; f'
alias cmdr='cargo make db-reset'
alias cmdp='cargo make db-prepare'
alias cmdm='cargo make db-migrate'
alias cmf='cargo make format'
alias cmr='cargo make run'

ksecdec () {
  kubectl get secrets -oname ${3:+--namespace=$3} | \
    rg "secret/(.*$2.*)" -r '$1' | xargs -I {} kubectl get secret {} -oyaml | \
    rg "\s+(.*$1.*):\s+(.*)" -r '$1:$2' | \
    while read -r kv
    do
      dv=$(echo "$kv" | rg ".*:(.*)" -r '$1' | base64 -D)
      k=$(echo "$kv" | rg "(.*):.*" -r '$1')
      echo "$k" "$dv"
    done
}

kcronrest () {
  maybe_namespace=${2:+--namespace=$2}
  kubectl get cronjobs "$1" "$maybe_namespace" --export -oyaml > foo.yaml
  kubectl delete cronjobs -f "$1" "$maybe_namespace" --ignore-not-found
  kubectl apply -f "$maybe_namespace" foo.yaml
}

# FFS 😩
[ -e "$HOME/.rover/env" ] && . "$HOME/.rover/env"

# GIGACHAD 🦾
[ -e "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
