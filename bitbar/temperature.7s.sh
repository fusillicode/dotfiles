#!/bin/bash

cpu_temperature=$(~/bin/smc -k TC0P -r | grep -o '[0-9]\+\.')
echo "${cpu_temperature%?}℃ | size=10"
echo "---"
fan_speed=$(~/bin/smc -k F0Ac -r | grep -o '\s\s[0-9]\+')
echo "$fan_speed RPM | size=12 color=white"
