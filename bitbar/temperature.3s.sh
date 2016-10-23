#!/bin/sh

cpu_temperature=$(/usr/local/bin/smc -k TC0P -r | grep -o '[0-9]\+\.')

echo "${cpu_temperature%?}°c | size=12"
