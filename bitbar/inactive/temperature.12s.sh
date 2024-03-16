#!/usr/bin/env bash

cpu_temperature=$(/Applications/smcFanControl.app/Contents/Resources/smc -k TC0P -r | /opt/homebrew/bin/rg ".*\s([0-9]{2})\..*" -r '$1')
battery_temperature=$(/Applications/smcFanControl.app/Contents/Resources/smc -k TB0T -r | /opt/homebrew/bin/rg ".*\s([0-9]{2})\..*" -r '$1')
echo "${cpu_temperature%}℃  ${battery_temperature%}℃ | size=12"
