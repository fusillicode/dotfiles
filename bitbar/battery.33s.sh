#!/usr/bin/env bash

battery_info=$(system_profiler SPPowerDataType)

charge_remaining=$(echo "$battery_info" | ~/.cargo/bin/rg "State of Charge" | awk '{print $5}')
echo "⚡︎ $charge_remaining% | size=12"
echo "---"
cycle_count=$(echo "$battery_info" | ~/.cargo/bin/rg "Cycle Count" | awk '{print $3}')
echo "$cycle_count cycles | size=12 color=white"
maximum_capacity=$(echo "$battery_info" | ~/.cargo/bin/rg "Maximum Capacity" | awk '{print $3}')
echo "Max capacity: $maximum_capacity | size=12 color=white"
condition=$(echo "$battery_info" | ~/.cargo/bin/rg "Condition" | sed -e 's/^.*: //')
echo "Battery condition: $condition | size=12 color=white"
