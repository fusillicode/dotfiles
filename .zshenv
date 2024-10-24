alias nv="nvim"
alias code="code-insiders"
alias gs="git status"
alias h="atuin history list"
alias l="ls -llAtrh"
alias j="jq . -c"
alias jl="jq . "
alias gcnuke="git commit --amend --no-edit --no-verify --allow-empty && git push --force-with-lease --no-verify"
alias gcnoke="git commit --amend --no-edit --no-verify --allow-empty"
alias gbrr="git for-each-ref --sort=committerdate refs/heads/ --format='%(HEAD) %(color:yellow)%(refname:short)%(color:reset) %(color:red)%(objectname:short)%(color:reset) - %(contents:subject) %(color:green)(%(committerdate:relative))%(color:reset) %(color:blue)<%(authorname)>%(color:reset)'"
alias gdh="git diff HEAD~1"
alias gfp="git fetch --all --prune && git pull"
alias kdebian="kubectl exec -it debian -- bash || kubectl run debian --image=debian:latest --rm -it --restart=Never --command --"
alias carbo="cargo"
alias cmc="cargo make clippy"
alias cmch="cargo make check"
alias cmt="cargo make test"
alias cmdr="cargo make db-reset"
alias cmdp="cargo make db-prepare"
alias cmf="cargo make format"

# Easy Shell commands
sorce () {
  set -a && source "$@" && set +a
}

# Easy Git
gtnuke () {
  git tag -f "$1" && git push origin "$1" -f
}

gt () {
  git --no-pager tag
  git ls-remote --tags
}

gmm () {
  git commit -m $1
}

# Easy K8S
klfir() {
  kubectl get pods | rg "$1" | head -n 1 | rg "^(\S*).*" -r '$1' | xargs -I {} kubectl logs -f {} "$2"
}

kseclist () {
  k get secrets -oname ${1:+--namespace=$1}
}

ksecyaml () {
  k get secrets -oname ${2:+--namespace=$2} | rg "secret/(.*$1.*)" -r '$1' | xargs -I {} kubectl get secret {} -oyaml
}

ksecdec () {
  k get secrets -oname ${3:+--namespace=$3} | \
    rg "secret/(.*$2.*)" -r '$1' | xargs -I {} kubectl get secret {} -oyaml | \
    rg "\s+(.*$1.*):\s+(.*)" -r '$1:$2' | \
    while read -r kv
    do
      dv=$(echo "$kv" | rg ".*:(.*)" -r '$1' | base64 -D)
      k=$(echo "$kv" | rg "(.*):.*" -r '$1')
      echo "$k" "$dv"
    done
}

kcronsus () {
  k patch cronjobs "$1" --patch '{"spec": {"suspend": '"$2"'}}'
}

kcronrest () {
  maybe_namespace=${2:+--namespace=$2}
  k get cronjobs "$1" "$maybe_namespace" --export -oyaml > foo.yaml
  k delete cronjobs -f "$1" "$maybe_namespace" --ignore-not-found
  k apply -f "$maybe_namespace" foo.yaml
}

kdeplscale () {
  k patch deployment "$1" --patch '{"spec": {"replicas": '"$2"'}}'
}

kdelerrpod () {
  kubectl get pods | rg "(\S+).*Error.*" -r '$1' | xargs -I {} kubectl delete pod {}
}

# Easy Postgres
pg_copy_table() {
  pg_dump -a -t "$1" "$2" | psql "$3"
}

# FFS 😩
[ -e "$HOME/.rover/env" ] && . "$HOME/.rover/env"

# GIGACHAD 🦾
[ -e "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
