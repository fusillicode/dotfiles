#!/bin/sh

memory_pressure=$(memory_pressure)
system_wide_memory_free_percentage=$(memory_pressure | grep 'System-wide memory free percentage' | grep -o '[0-9]\+')
echo "${system_wide_memory_free_percentage}% | size=11"
