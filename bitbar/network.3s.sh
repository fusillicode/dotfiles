#!/bin/sh

export PATH="/usr/local/bin:${PATH}"
INTERFACES=$(ifconfig -lu)

echo "$(ifstat -n -w -i en0 0.1 1 | tail -n 1 | awk '{printf "%0.2f ▼ %0.2f ▲", $1/128, $2/128;}') | size=11"
echo "---"

for INTERFACE in ${INTERFACES}; do
  if [[ ${INTERFACE} != "en0" ]]; then
    echo "$(ifstat -n -w -i "${INTERFACE}" 0.1 1 | tail -n 1 | awk '{printf "%0.2f ▼ %0.2f ▲", $1/128, $2/128;}') (${INTERFACE})"
  fi
done
