#!/bin/bash

export PATH="/usr/local/bin:${PATH}"

echo "$(ifstat -n -w -i en0 0.1 1 | tail -n 1 | awk '{printf "%0.2f ▼ %0.2f ▲", $1/128, $2/128;}') | size=10"

echo "---"
for interface in $(ifconfig -lu); do
  if [[ ${interface} != "en0" ]]; then
    echo "$(ifstat -n -w -i "${interface}" 0.1 1 | tail -n 1 | awk '{printf "%0.2f ▼ %0.2f ▲", $1/128, $2/128;}') (${interface}) | size=12"
  fi
done
