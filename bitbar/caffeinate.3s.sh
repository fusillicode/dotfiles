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

echo $(icon)

echo "---"
echo "Caffeinate | size=12 bash='$0' param1=start terminal=false"
echo "Decaffeinate | size=12 bash='$0' param1=stop terminal=false"
