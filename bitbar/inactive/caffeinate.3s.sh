#!/bin/bash

function icon {
  if [[ -z "$(pgrep caffeinate)" ]]; then echo "🍵"; else echo "☕️"; fi
}

function terminate_caffeinate_instances {
  /usr/bin/killall caffeinate &> /dev/null
}

if [[ "$1" = "start" ]]; then
  terminate_caffeinate_instances;
  /usr/bin/caffeinate -di;
  exit
fi

if [[ "$1" = "stop" ]]; then
  terminate_caffeinate_instances;
  exit
fi

icon
echo "---"
if [[ -z "$(pgrep caffeinate)" ]]; then
  echo "Caffeinate | size=12 color=white bash='$0' param1=start terminal=false"
else
  echo "Decaffeinate | size=12 color=white bash='$0' param1=stop terminal=false"
fi
