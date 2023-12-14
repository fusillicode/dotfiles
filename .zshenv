alias nv="nvim"
alias code="code-insiders"
alias gs="git status"
alias h="atuin history list -f '{time} | {command}'"
alias l="ls -latrh"
alias j="jq . -c"
alias jl="jq . "
alias gcnuke="git commit --amend --no-edit --no-verify --allow-empty && git push --force-with-lease --no-verify"
alias gcnoke="git commit --amend --no-edit --no-verify --allow-empty"
alias kdebian="kubectl exec -it debian -- bash || kubectl run debian --image=debian:latest --rm -it --restart=Never --command --"

# Easy Shell commands
sorce () {
  set -a && source "$@" && set +a
}

ho () {
  ~/bin/weh ho "$@"
}

# Easy Git
gtnuke () {
  git tag -f "$1" && git push origin "$1" -f
}

gt () {
  git --no-pager tag
  git ls-remote --tags
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
source "$HOME/.rover/env"
