#!/bin/bash

battery_info=$(system_profiler SPPowerDataType)

charge_remaining=$(echo "$battery_info" | grep "Charge Remaining" | awk '{print $4}')
echo "$charge_remaining ⚡︎ | size=10"

echo "---"
cycle_count=$(echo "$battery_info" | grep "Cycle Count" | awk '{print $3}')
echo "$cycle_count cycles | size=12"
condition=$(echo "$battery_info" | grep "Condition" | sed -e 's/^.*: //')
echo "Battery condition: $condition | size=12"
