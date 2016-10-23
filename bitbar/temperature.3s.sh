#!/bin/sh

cpu_temperature=$(/usr/local/bin/smc -k TC0P -r | grep -o '[0-9]\+\.')
echo "${cpu_temperature%?}°c | size=12"

echo "---"
fan_speed=$(/usr/local/bin/smc -f | grep 'Actual speed' | grep -o '[0-9]\+')
echo "$fan_speed RPM"
