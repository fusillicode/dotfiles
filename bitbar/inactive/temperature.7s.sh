#!/bin/bash

cpu_temperature=$(/Applications/smcFanControl.app/Contents/Resources/smc -k TC0P -r | grep -o '[0-9]\+\.')
echo "${cpu_temperature%?}℃ | size=10"
echo "---"
fan_speed=$(/Applications/smcFanControl.app/Contents/Resources/smc -k F0Ac -r | grep -o '\s\s[0-9]\+')
echo "$fan_speed RPM | size=12 color=white"
