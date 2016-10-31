#!/bin/sh

cpu_temperature=$(~/bin/smc -k TC0P -r | grep -o '[0-9]\+\.')
echo "${cpu_temperature%?}°c | size=11"

echo "---"
fan_speed=$(~/bin/smc -f | grep 'Actual speed' | grep -o '[0-9]\+')
echo "$fan_speed RPM"
