#!/bin/sh

memory_pressure=$(memory_pressure)
system_wide_memory_free_percentage=$(memory_pressure | grep 'System-wide memory free percentage' | grep -o '[0-9]\+')
system_wide_used_memory_percentage=$((100 - $system_wide_memory_free_percentage))
echo "${system_wide_used_memory_percentage}% | size=11"
