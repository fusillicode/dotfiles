#!/bin/sh

battery_info=$(system_profiler SPPowerDataType)

cycle_count=$(echo "$battery_info" | grep "Cycle Count" | awk '{print $3}')
echo "$cycle_count | size=12"

echo "---"

charge_remaining=$(echo "$battery_info" | grep "Charge Remaining" | awk '{print $4}')
echo "$charge_remaining mAh"

condition=$(echo "$battery_info" | grep "Condition" | sed -e 's/^.*: //')
echo "Battery condition: $condition"
