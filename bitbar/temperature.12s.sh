#!/bin/bash

cpu_temperature=$(/Applications/smcFanControl.app/Contents/Resources/smc -k TC0P -r | grep -o '[0-9]\+\.')
battery_temperature=$(/Applications/smcFanControl.app/Contents/Resources/smc -k TB0T -r | grep -o '[0-9]\+\.')
echo "${cpu_temperature%?}℃  ${battery_temperature%?}℃ | size=12"
