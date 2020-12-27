#!/bin/bash

battery_info=$(system_profiler SPPowerDataType)

charge_remaining=$(echo "$battery_info" | /usr/local/bin/rg "Full Charge Capacity" | awk '{print $5}')
echo "$charge_remaining ⚡︎ | size=12"
echo "---"
cycle_count=$(echo "$battery_info" | /usr/local/bin/rg "Cycle Count" | awk '{print $3}')
echo "$cycle_count cycles | size=12 color=white"
condition=$(echo "$battery_info" | /usr/local/bin/rg "Condition" | sed -e 's/^.*: //')
echo "Battery condition: $condition | size=12 color=white"
